//! Unit testing framework for an initial operational transform implementation.
//! Simulates a network which processes events within and between servers and clients via a single,
//! time-ordered event loop.
//!
//! Maintains a global clock and sends messages to all connected components at a given global time.
//! Allows simulating message passing and arbitrary network delays.

mod bindings;
mod cursor;
mod user;

pub use bindings::*;
pub use cursor::*;
pub use user::*;

use anyhow::Result;
use fnv::FnvHashMap;
use std::collections::binary_heap::BinaryHeap;
use std::{cell::RefCell, cmp::Reverse, rc::Rc};
use thiserror::Error;

use crate::{client::ClientNetwork, Snapshot};

// Unique ID of a component in the network
pub type ComponentId = u64;
type Time = u64;
pub type EventQueue = BinaryHeap<Reverse<NetworkEvent>>;
type MetadataMap = FnvHashMap<ComponentId, ComponentMetadata>;

/// Represents both a "network" in the traditional sense of a medium to pass messages between different components,
/// but also processes local events on the same component. More generally, this represents an event processor that
/// simulates scheduled events by time, some of which are local and others of which are cross component. All messages
/// sent are [`NetworkMessage`]s, and the distinction between messages sent to the same or different components is made
/// by [`LocalMessage`] and [`RemoteUpdate`], respectively.
pub struct Network {
    server_id: Rc<RefCell<Option<ComponentId>>>,
    client_ids: Rc<RefCell<Vec<ComponentId>>>,
    components: FnvHashMap<ComponentId, Box<dyn Component>>,
    component_metadata: Rc<RefCell<MetadataMap>>,
    clock: Rc<RefCell<Time>>,
    events: Rc<RefCell<EventQueue>>,
    next_id: ComponentId,
    fired_events: u64,
    network_state: NetworkState,
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn new() -> Self {
        Self {
            server_id: Rc::new(RefCell::new(None)),
            client_ids: Rc::new(RefCell::new(Vec::new())),
            components: Default::default(),
            component_metadata: Rc::new(RefCell::new(FnvHashMap::default())),
            clock: Rc::new(RefCell::new(0)),
            events: Rc::new(RefCell::new(BinaryHeap::new())),
            next_id: 0,
            fired_events: 0,
            network_state: NetworkState::Initializing,
        }
    }

    /// Jumps forward to the next message sorted ascending by time
    fn next_message(&mut self) -> Result<Option<NetworkMessage>, NetworkError> {
        if self.server_id.borrow().is_none() {
            return Err(NetworkError::NoServer);
        }
        // No panic: Exclusive borrow gives exclusive access to the event queue
        let next_event = self.events.borrow_mut().pop();
        if let Some(Reverse(NetworkEvent {
            message,
            time: receive_time,
        })) = next_event
        {
            *self.clock.borrow_mut() = receive_time;
            self.fired_events += 1;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    // Advances the network until all events at the next greatest timestamp is processed (can advance by more than one event).
    fn tick(&mut self) -> Result<NetworkState, NetworkError> {
        if self.server_id.borrow().is_none() {
            return Err(NetworkError::NoServer);
        }

        let mut tick_time = None;

        loop {
            if let Some(event) = self.events.borrow().peek() {
                if tick_time.is_none() {
                    tick_time = Some(event.0.time);
                }
                if event.0.time != tick_time.unwrap() {
                    self.network_state = NetworkState::Running;
                    return Ok(self.network_state);
                }
            } else {
                self.network_state = NetworkState::Stopped;
                return Ok(self.network_state);
            }
            // Peek showed there is a message
            let event = self.next_message()?.unwrap();

            match event {
                NetworkMessage::Local(local) => {
                    let client_id = local.client_id;
                    let component = self.components.get_mut(&client_id).unwrap();
                    component.local_update(local);
                }
                NetworkMessage::Remote(remote) => {
                    let dest = remote.dest;
                    let component = self.components.get_mut(&dest).unwrap();
                    component.remote_update(remote)?;
                }
                NetworkMessage::LateJoiner(client_config) => {
                    let component = Box::new(ClientNetwork::new(self));
                    let late_joiner = true;
                    self.add_component(component, client_config.delay, late_joiner)?;
                }
                NetworkMessage::Snapshot(snapshot) => {
                    let dest = snapshot.dest;
                    let component = self.components.get_mut(&dest).unwrap();
                    component.recv_snapshot(snapshot);
                }
            }
        }
    }

    /// Runs the network until all events are processed.
    pub fn run(&mut self) -> Result<NetworkState, NetworkError> {
        while self.tick()? != NetworkState::Stopped {}
        debug_assert!(self.network_state == NetworkState::Stopped);
        Ok(self.network_state)
    }

    /// Preschedule an event to be sent to a component at a given time. Call this before running the network.
    /// In OT, these represent the set of client "operation generation" events, as opposed to "operation reception" events,
    /// which are scheduled by the components and network during running.
    ///
    /// This is used to simulate the client generating an operation locally.
    pub fn schedule_local_message(
        &mut self,
        message: LocalMessage,
        send_time: Time,
    ) -> Result<(), NetworkError> {
        if !self.components.contains_key(&message.client_id) {
            return Err(NetworkError::ComponentNotFound(message.client_id));
        }
        // No panic: Exclusive borrow gives exclusive access to the event queue
        self.events.borrow_mut().push(Reverse(NetworkEvent {
            message: NetworkMessage::Local(message),
            time: send_time,
        }));
        Ok(())
    }

    /// Entry point for components to send messages to other network components by scheduling events on a network.
    /// Conceptually this is the `send` function for a [`Component`].
    pub fn schedule_remote_message(
        clock: Rc<RefCell<Time>>,
        events: Rc<RefCell<EventQueue>>,
        component_metadata: Rc<RefCell<FnvHashMap<ComponentId, ComponentMetadata>>>,
        message: RemoteUpdate,
    ) {
        let dest = message.dest;
        let receive_time = *clock.borrow()
            + component_metadata.borrow()[&dest]
                .delay
                .delay(message.source);
        events.borrow_mut().push(Reverse(NetworkEvent {
            message: NetworkMessage::Remote(message),
            time: receive_time,
        }));
    }

    /// Entry point for the server to send a snapshot to client with ID `client_id`. Used to synchronize late joiners.
    pub fn schedule_snapshot(
        clock: Rc<RefCell<Time>>,
        events: Rc<RefCell<EventQueue>>,
        server_id: ComponentId,
        client_id: ComponentId,
        component_metadata: Rc<RefCell<FnvHashMap<ComponentId, ComponentMetadata>>>,
        snapshot: Snapshot,
        apply_delay: bool,
    ) {
        // `apply_delay` is only set for snapshots to late joiners
        let receive_time = if apply_delay {
            *clock.borrow()
                + component_metadata.borrow()[&client_id]
                    .delay
                    .delay(server_id)
        } else {
            *clock.borrow()
        };
        events.borrow_mut().push(Reverse(NetworkEvent {
            message: NetworkMessage::Snapshot(snapshot.clone()),
            time: receive_time,
        }));
    }

    /// Schedule a new client to be added at a given time, simulates a late joiner
    pub fn schedule_late_joiner(&mut self, clock: Time, client_config: ClientConfig) {
        self.events.borrow_mut().push(Reverse(NetworkEvent {
            message: NetworkMessage::LateJoiner(client_config),
            time: clock,
        }));
    }

    /// `add_component` but the component joins on time.
    pub fn add_component_sod(
        &mut self,
        component: Box<dyn Component>,
        delay: Delay,
    ) -> Result<(), NetworkError> {
        if self.network_state != NetworkState::Initializing {
            return Err(NetworkError::CannotAddSodComponent);
        }
        self.add_component(component, delay, false)
    }

    /// Initialize the network by adding components. Call this before running the network.
    /// `delay` is the number of time units to add to the send time of inbound messages to a component
    /// before the component receives any messages.
    ///
    /// The server must be added before any clients.
    fn add_component(
        &mut self,
        component: Box<dyn Component>,
        delay: Delay,
        late_joiner: bool,
    ) -> Result<(), NetworkError> {
        if self.components.contains_key(&component.id()) {
            return Err(NetworkError::DuplicateComponentId(component.id()));
        }

        // Server must be added before any clients
        if matches!(component.kind(), ComponentKind::Client) && self.server_id.borrow().is_none() {
            return Err(NetworkError::NoServer);
        }

        let id = component.id();

        let component_metadata = ComponentMetadata { delay };
        // No panic: Exclusive borrow gives exclusive access to the metadata map
        // Must insert component metadata first because `new_client` uses metadata delay
        // to schedule first client snapshot
        debug_assert!(self
            .component_metadata
            .borrow_mut()
            .insert(id, component_metadata)
            .is_none());

        let kind = component.kind();
        if kind == ComponentKind::Server {
            if self.server_id.borrow().is_some() {
                return Err(NetworkError::DuplicateServer);
            }
            *self.server_id.borrow_mut() = Some(id);
        } else {
            self.client_ids.borrow_mut().push(id);
            // Schedule a new client message to the server.
            // Sends a snapshot (effectively a noop since the server document is empty)
            // but main function is to add the client to the server as a User.
            self.components
                .get_mut(&self.server_id.borrow().unwrap())
                .unwrap()
                .new_client(id, late_joiner);
        }
        debug_assert!(self.components.insert(id, component).is_none());
        Ok(())
    }

    /// Get the next available component ID and increment the ID counter.
    pub fn next_id(&mut self) -> ComponentId {
        let next_id = self.next_id;
        self.next_id += 1;
        next_id
    }

    pub fn client_ids(&self) -> Rc<RefCell<Vec<ComponentId>>> {
        self.client_ids.clone()
    }

    /// Server ID will be defined after `run()` is called. May be uninitialized before then.
    pub fn server_id(&self) -> Rc<RefCell<Option<ComponentId>>> {
        self.server_id.clone()
    }

    pub fn network_shared(&self) -> NetworkShared {
        NetworkShared {
            server_id: self.server_id.clone(),
            client_ids: self.client_ids.clone(),
            clock: self.clock.clone(),
            events: self.events.clone(),
            component_metadata: self.component_metadata.clone(),
        }
    }

    pub fn fired_events(&self) -> u64 {
        self.fired_events
    }

    pub fn component(&self, id: ComponentId) -> Option<&dyn Component> {
        self.components.get(&id).map(|c| c.as_ref())
    }

    pub fn time(&self) -> Time {
        *self.clock.borrow()
    }
}

pub struct NetworkShared {
    server_id: Rc<RefCell<Option<ComponentId>>>,
    client_ids: Rc<RefCell<Vec<ComponentId>>>,
    clock: Rc<RefCell<Time>>,
    events: Rc<RefCell<EventQueue>>,
    component_metadata: Rc<RefCell<FnvHashMap<ComponentId, ComponentMetadata>>>,
}

impl NetworkShared {
    pub fn server_id(&self) -> Rc<RefCell<Option<ComponentId>>> {
        self.server_id.clone()
    }

    pub fn client_ids(&self) -> Rc<RefCell<Vec<ComponentId>>> {
        self.client_ids.clone()
    }

    pub fn clock(&self) -> Rc<RefCell<Time>> {
        self.clock.clone()
    }

    pub fn events(&self) -> Rc<RefCell<EventQueue>> {
        self.events.clone()
    }

    pub fn component_metadata(&self) -> Rc<RefCell<FnvHashMap<ComponentId, ComponentMetadata>>> {
        self.component_metadata.clone()
    }
}

// Light wrapper around a component. Incorporates delays for scheduling events.
pub struct ComponentMetadata {
    // Represents the delay from the sender to this component as a receiver.
    // Realistically, this value would be different per message, but for simplicitly this
    // sets one constant value for a given component.
    delay: Delay,
}

/// Represents constant and variable delays. Needed to simulate more complex messaging orders.
#[derive(Debug)]
pub enum Delay {
    // Constant delay for all messages from all components
    Constant(u64),
    // Variable delay depending on the source component
    Variable {
        // Custom per component delay
        per_component: FnvHashMap<ComponentId, Time>,
        // If no component delay is specified for a component, delay defaults to this value
        default: Time,
    },
}

impl Delay {
    /// Get the delay for a message from a source component
    pub fn delay(&self, source: ComponentId) -> Time {
        match self {
            Delay::Constant(delay) => *delay,
            Delay::Variable {
                per_component,
                default,
            } => *per_component.get(&source).unwrap_or(default),
        }
    }

    pub fn constant(delay: u64) -> Self {
        Delay::Constant(delay)
    }

    pub fn variable<T: IntoIterator<Item = (ComponentId, Time)>>(
        per_component: T,
        default: Time,
    ) -> Self {
        let per_component = per_component.into_iter().collect();
        Delay::Variable {
            per_component,
            default,
        }
    }
}

pub trait Component {
    /// A message sent from a different component. Conceptually, a message received "across a network" with associated delays.
    fn remote_update(&mut self, remote_update: RemoteUpdate) -> Result<()>;
    /// Receive a snapshot from the server, used for late joiners
    fn recv_snapshot(&mut self, snapshot: Snapshot);
    /// Represents a local client update. This is not a message received "across the network" in a real application.
    /// A [`Component`] does not have a `send` method because only the side effects of `local_updates` trigger sending
    /// messages to other components.
    fn local_update(&mut self, message: LocalMessage);
    /// Callback for server when new client is added. Should only be implemented by server.
    fn new_client(&mut self, component_id: ComponentId, late_joiner: bool);
    fn id(&self) -> ComponentId;
    fn kind(&self) -> ComponentKind;
    fn document(&self) -> &str;
    /// Queries the components' cursor map for the cursor position of a user. Returns the [`CursorPos`] if one exists.
    fn cursor_pos(&self, user_id: UserId) -> Option<CursorPos>;
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub enum ComponentKind {
    Server,
    Client,
}

/// Time aware [`NetworkMessage`]. Conceptually, `messages` have data and `events` are timestampped messages by a global clock.
#[derive(Debug)]
pub struct NetworkEvent {
    message: NetworkMessage,
    time: Time,
}

impl PartialEq for NetworkEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for NetworkEvent {}

// All `NetworkEvent` messages are comparable to each other because there is a total ordering based on the global clock.
impl PartialOrd for NetworkEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NetworkEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.time.cmp(&other.time) {
            std::cmp::Ordering::Equal => self
                .message
                .network_message_order()
                .cmp(&other.message.network_message_order()),
            ordering => ordering,
        }
    }
}

/// Represents an event the network struct processes. Not all events are "transmitted across a network"
/// in the sense that they are sent between two components. Some events are local to a component (e.g. a users
/// updating their local document) so they only have a `source`. These are still processed by the network event
/// loop so they are considered network events.
/// Time not specified as a field by client/server so the network can add delays
#[derive(Debug)]
pub enum NetworkMessage {
    Local(LocalMessage),
    Remote(RemoteUpdate),
    // Sent only for late joiners. IDs are assigned in increasing order based on creation time.
    LateJoiner(ClientConfig),
    // Server sends snapshot to late joiners
    Snapshot(Snapshot),
}

impl NetworkMessage {
    pub(crate) fn network_message_order(&self) -> u8 {
        // Establishes ordering of enum variants for sorting.
        // [`NetworkMessage::Snapshot`] should come before all other message types when occurring at the same time
        // such that client which join the network at `t=0` do not have an empty snapshot override a concurrent client edit at `t=0`.
        match self {
            NetworkMessage::Local(_) => 1,
            NetworkMessage::Remote(_) => 1,
            NetworkMessage::LateJoiner(_) => 1,
            NetworkMessage::Snapshot(_) => 0,
        }
    }
}

/// Represents a user generated event (e.g. updating their local document). This only affects the local component,
/// has no delay, and has no knowledge of `state_id` which is assigned when a [`RemoteUpdate`] is triggered.
/// `source = dest = client_id` for local messages.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LocalMessage {
    pub client_id: ComponentId,
    /// Raw document state after a user has updated their local copy. Does not involve [`TextOperation`]s which is
    /// an abstraction internal to the [`Client`] used to broadcast transformations updates.
    pub document: String,
    /// Cursor position after document update or changing cursor selection
    pub cursor_pos: CursorPos,
}

#[derive(Debug)]
pub struct ClientConfig {
    delay: Delay,
}

/// Represents the network state. Once `tick()` is called, the network transitions to [`NetworkState::Running`].
/// Returned by `run` and `tick` to indicate network state after it starts running
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Initializing,
    Running,
    Stopped,
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Component with ID {0} already exists")]
    DuplicateComponentId(ComponentId),
    #[error("Component with ID {0} not found")]
    ComponentNotFound(ComponentId),
    #[error("Duplicate server, only one server allowed")]
    DuplicateServer,
    #[error("No server")]
    NoServer,
    #[error("Components must be added as late joiners after the network has started running")]
    CannotAddSodComponent,
    #[error(transparent)]
    AnyhowError(#[from] anyhow::Error),
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::client::ClientNetwork;
    use crate::server::ServerNetwork;

    // Test notation:
    // Ln       - local message number n from a client
    // O (Si)   - A local message (possibly transformed) broadcast to the server relative to historical server state Si
    // Ack (Ln) - Client receiving server acknowledgement that server has applied client's operation Ln
    // S        - server message, received by a client
    // Snap     - server message, received by a client, special case of snapshot of server state when client joins
    // Sn       - server state (only applies to the server component), denotes reference server state a client output transformation
    //            is relative to
    // C1, Cn   - client 1, client n
    // ...      - After some time, the listed operations will occur
    // "s|tr |" - `|` represents the begin and end of a cursor highlight

    // Test naming terminology:
    // - single/multi client: Number of clients connected to server (multi client tests collaboration)
    // - sequential: no client messages are bufferred, clients may have at most 1 outstanding operation that is not acked by the server
    // - buffered: If a client sends multiple operations before receiving an ack from the server, messages are buffered
    //             tests logic that the buffer is transformed by the server operations
    // - multiops: tests insert, delete, and retain operations
    // - multilingual: text is in a language where each character is multiple bytes
    //  (tests tracking is in units of characters, not bytes, or even grapheme characters)
    mod cursor_tests {
        // Tests designed primarily for cursor tracking.
        // These will still test text transformations as a side effect, but these explicitly assert cursor positions.
        use super::*;

        #[test]
        fn test_single_client_cursor_move() {
            // The client adds text, then moves the cursor without changing the text.
            // Time    C1                      Server
            // 0       Snap  - ""
            // 0       L1    - "hello|"
            // 1                               C1 -> "hello|"
            // 2       Ack(L1)
            // 3       L2    - "he|llo"
            // 4                               C1 -> "he|llo"
            // 5       Ack(L2)
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();

            let start_text = "";
            // Cursor: "hello|"
            let client1_edit1 = "hello";
            let client1_cursor_pos1 = CursorPos::new(5, 5, 5, 5);
            // Cursor: "he|llo"
            let client1_edit2 = "hello";
            let client1_cursor_pos2 = CursorPos::new(2, 2, 2, 2);
            let server_state1 = client1_edit1;
            let server_state2 = client1_edit2;

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_pos1,
            };

            let event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_pos2,
            };

            network.schedule_local_message(event1, 0).unwrap();
            network.schedule_local_message(event2, 3).unwrap();

            // T = 0
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos1)
            );

            // T = 1, T = 2 (Ack)
            for t in [1, 2] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state1
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos1),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit1
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos1)
                );
            }

            // T = 3
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 3);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos1),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos2)
            );

            // T = 4, T = 5 (Ack)
            for t in [4, 5] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state2
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos2),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit2
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos2)
                );
            }

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_single_client_cursor_highlight_multiop() {
            // Test single client changing a cursor highlight range with multiple operations (i.e. delete, retain, insert).
            // No operations are buffered.
            // Some of these operations are not "possible" with a single operation on a real
            // text editor via keyboard input assuming the text editor sends updates every character change or highlight
            // change. Instead, they are multiple operations. For example, `h|ello|` -> `h|e|` requires a delete then rehighlight.
            // This should be strictly "harder" than assuming only single ops, so this is a further test of robustness.
            // Note: in a given highlight range, for simplicitly, the anchor is a smaller index than the head.
            //
            // Time     C1                      Server
            // 0        Snap  - ""
            // 1        L1    - "hello|"
            // 2                                C1 -> "hello|"
            // 3        Ack(L1)
            // 4        L2    - "h|ello|"
            // 5                                C1 -> "h|ello|"
            // 6        Ack(L2)
            // 7        L3    - "h|e|"
            // 8                                C1 -> "h|e|"
            // 9        Ack(L3)
            // 10       L4    - "|"
            // 11                               C1 -> "|"
            // 12       Ack(L4)
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();

            let start_text = "";
            // Cursor: "hello|"
            let client1_edit1 = "hello";
            let client1_cursor_pos1 = CursorPos::new(5, 5, 5, 5);
            // Cursor: "h|ello|"
            let client1_edit2 = "hello";
            let client1_cursor_pos2 = CursorPos::new(1, 5, 1, 5);
            // Cursor: "h|e|"
            let client1_edit3 = "he";
            let client1_cursor_pos3 = CursorPos::new(1, 2, 1, 2);
            // Cursor: "|"
            let client1_edit4 = "";
            let client1_cursor_pos4 = CursorPos::new(0, 0, 0, 0);
            let server_state1 = client1_edit1;
            let server_state2 = client1_edit2;
            let server_state3 = client1_edit3;
            let server_state4 = client1_edit4;

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_pos1,
            };

            let event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_pos2,
            };

            let event3 = LocalMessage {
                client_id: client1_id,
                document: client1_edit3.to_string(),
                cursor_pos: client1_cursor_pos3,
            };

            let event4 = LocalMessage {
                client_id: client1_id,
                document: client1_edit4.to_string(),
                cursor_pos: client1_cursor_pos4,
            };

            network.schedule_local_message(event1, 1).unwrap();
            network.schedule_local_message(event2, 4).unwrap();
            network.schedule_local_message(event3, 7).unwrap();
            network.schedule_local_message(event4, 10).unwrap();

            // T = 0 (Snapshot)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);

            // T = 1
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 1);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos1)
            );

            // T = 2, T = 3 (Ack)
            for t in [2, 3] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state1
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos1),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit1
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos1)
                );
            }

            // T = 4
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 4);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos1),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos2)
            );

            // T = 5, T = 6 (Ack)
            for t in [5, 6] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state2
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos2),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit2
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos2)
                );
            }

            // T = 7
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 7);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos2),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos3)
            );

            // T = 8, T = 9 (Ack)
            for t in [8, 9] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state3
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos3),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit3
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos3)
                );
            }

            // T = 10
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos3),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 11, T = 12 (Ack)
            for t in [11, 12] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state4
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(client1_cursor_pos4),
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit4
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_pos4)
                );
            }
        }

        #[test]
        fn test_single_client_cursor_highlight_multiop_buffered() {
            // Identical to `test_single_client_cursor_highlight_multiop` but all client operations are buffered.
            //
            // Time     C1                      Server
            // 0        Snap  - ""
            // 1        L1/O (S0) - "hello|"
            // 2        L2    - "h|ello|"
            // 3        L3    - "h|e|"
            // 4        L4    - "|"
            // 5                                C1/S1 -> "hello|"
            // 6        Ack(L1)
            //          O (S1) - L2
            // 10                               C1/S2 -> "h|ello|"
            // 11       Ack(L2)
            //          O (S2) - L3
            // 15                               C1/S3 -> "h|e|"
            // 16       Ack(L3)
            //          O (S3) - L4
            // 20                               C1/S4 -> "|"
            // 21       Ack(L4)
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(4))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();

            let start_text = "";
            // Cursor: "hello|"
            let client1_edit1 = "hello";
            let client1_cursor_pos1 = CursorPos::new(5, 5, 5, 5);
            // Cursor: "h|ello|"
            let client1_edit2 = "hello";
            let client1_cursor_pos2 = CursorPos::new(1, 5, 1, 5);
            // Cursor: "h|e|"
            let client1_edit3 = "he";
            let client1_cursor_pos3 = CursorPos::new(1, 2, 1, 2);
            // Cursor: "|"
            let client1_edit4 = "";
            let client1_cursor_pos4 = CursorPos::new(0, 0, 0, 0);
            let server_state1 = client1_edit1;
            let server_state2 = client1_edit2;
            let server_state3 = client1_edit3;
            let server_state4 = client1_edit4;

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_pos1,
            };

            let event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_pos2,
            };

            let event3 = LocalMessage {
                client_id: client1_id,
                document: client1_edit3.to_string(),
                cursor_pos: client1_cursor_pos3,
            };

            let event4 = LocalMessage {
                client_id: client1_id,
                document: client1_edit4.to_string(),
                cursor_pos: client1_cursor_pos4,
            };

            network.schedule_local_message(event1, 1).unwrap();
            network.schedule_local_message(event2, 2).unwrap();
            network.schedule_local_message(event3, 3).unwrap();
            network.schedule_local_message(event4, 4).unwrap();

            // T = 0 (Snapshot)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);

            // T = 1
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 1);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos1)
            );

            // T = 2
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 2);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos2)
            );

            // T = 3
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 3);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos3)
            );

            // T = 4
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 4);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                None,
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos1),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 6 (and C1 Ack)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);

            // T = 10
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos2),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 11 (and C1 Ack)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 11);

            // T = 15
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 15);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos3),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 16 (and C1 Ack)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 16);

            // T = 20
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 20);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_pos4),
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit4
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_pos4)
            );

            // T = 21 (C1 Ack)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 21);

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_multi_client_cursor_highlight_multiop_sequential() {
            // Tests multiple clients changing cursor highlights with multiple operations in sequence (no buffering).
            // Time | C1                      | C2                          | Server
            //------|-------------------------|-----------------------------|------------------------
            // 0    | Snap - ""               | Snap - ""                   |
            // 1    | L1/O (S0) - "hello|"    |                             |
            // 2    |                         |                             | C1/S1 -> "hello|"
            // 3    | Ack(L1)                 | S1 - "hello|"               |
            // 4    |                         | L1/O (S1) - "hello world||" |
            // 5    | L2/O (S1) - "he|llo|"   |                             | C2/S2 -> "hello world||"
            // 6    | S2 - "he|llo| world|"   | Ack(L1)                     | C1/S3 -> "he|llo| world|" <- interesting C1, rebroadcasts range
            // 7    | Ack(L2)                 | S2 - "he|llo| world|"       |
            // 8    |                         | L2/O (S2) - "|world|"       |
            // 9    |                         |                             | C2/S4 -> "|world|"
            // 10   | S3 - "|world|"          | Ack(L2)                     |

            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let start_text = "";

            // Client 1
            // Cursor: "hello|"
            let client1_edit1 = "hello";
            let client1_cursor_edit1_pos = CursorPos::new(5, 5, 5, 5);
            // Cursor: "he|llo|"
            let client1_edit2 = "hello";
            let client1_cursor_edit2_pos = CursorPos::new(2, 5, 2, 5);
            // Cursor: "he|llo| world|"
            let client1_server_sync1 = "hello world";
            let client1_cursor_sync1_pos = CursorPos::new(2, 5, 2, 5);
            // Cursor: "|world|"
            let client1_server_sync2 = "world";
            let client1_cursor_sync2_pos = CursorPos::new(0, 0, 0, 0);

            // Client 2
            let client2_server_sync1 = "hello";
            // Client has not typed in document, so cursor is not present yet
            let client2_cursor_sync1_pos = None;
            // Cursor: "hello world||"
            let client2_edit1 = "hello world";
            let client2_cursor_edit1_pos = CursorPos::new(11, 11, 11, 11);
            let client2_edit1_c1_cursor_pos = CursorPos::new(11, 11, 11, 11);
            let client2_server_sync2 = "hello world";
            let client2_cursor_sync2_pos = client2_cursor_edit1_pos;
            // Cursor: "|world|"
            let client2_edit2 = "world";
            let client2_cursor_edit2_pos = CursorPos::new(5, 5, 5, 5);

            // Server
            let server_state1 = client1_edit1;
            let server_state2 = client2_edit1;
            let server_state3 = client2_edit1;
            let server_state4 = client2_edit2;

            let client1_event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_edit1_pos,
            };

            let client1_event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_edit2_pos,
            };

            let client2_event1 = LocalMessage {
                client_id: client2_id,
                document: client2_edit1.to_string(),
                cursor_pos: client2_cursor_edit1_pos,
            };

            let client2_event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit2.to_string(),
                cursor_pos: client2_cursor_edit2_pos,
            };

            network.schedule_local_message(client1_event1, 1).unwrap();
            network.schedule_local_message(client1_event2, 5).unwrap();
            network.schedule_local_message(client2_event1, 4).unwrap();
            network.schedule_local_message(client2_event2, 8).unwrap();

            // T = 0 - Snapshot
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);

            // T = 1
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 1);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                None,
            );

            // T = 2
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 2);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                None,
            );

            // T = 3
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 3);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                client2_cursor_sync1_pos,
            );

            // T = 4
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 4);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client2_edit1_c1_cursor_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 6
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 7
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 7);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync2_pos),
            );

            // T = 8, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 8);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            // T = 9, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 9);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            // T = 10, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_multi_client_cursor_highlight_multiop_sequential_multilingual() {
            // Tests similar to `test_multi_client_cursor_highlight_multiop_sequential` but with a language
            // where each character is multiple bytes.
            // Time | C1                      | C2                          | Server
            //------|-------------------------|-----------------------------|------------------------
            // 0    | Snap - ""               | Snap - ""                   |
            // 1    | L1/O (S0) - "你好|"      |                             |
            // 2    |                         |                             | C1/S1 -> "你好|"
            // 3    | Ack(L1)                 | S1 - "你好|"                 |
            // 4    |                         | L1/O (S1) - "你好世界||"      |
            // 5    | L2/O (S1) - "|你好|"     |                             | C2/S2 -> "你好世界||"
            // 6    | S2 - "｜你好|世界|"      | Ack(L1)                     | C1/S3 -> "｜你好|世界|" <- interesting C1, rebroadcasts range
            // 7    | Ack(L2)                 | S2 - "｜你好|世界|"          |
            // 8    |                         | L2/O (S2) - "|世界|"         |
            // 9    |                         |                             | C2/S4 -> "|世界|"
            // 10   | S3 - "|世界|"            | Ack(L2)                     |

            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let start_text = "";

            // Client 1
            // Cursor: "你好|"
            let client1_edit1 = "你好";
            let client1_cursor_edit1_pos = CursorPos::new(2, 2, 2, 2);
            // Cursor: "|你好|"
            let client1_edit2 = "你好";
            let client1_cursor_edit2_pos = CursorPos::new(0, 2, 0, 2);
            // Cursor: "|你好|世界|"
            let client1_server_sync1 = "你好世界";
            let client1_cursor_sync1_pos = CursorPos::new(0, 2, 0, 2);
            // Cursor: "|世界|"
            let client1_server_sync2 = "世界";
            let client1_cursor_sync2_pos = CursorPos::new(0, 0, 0, 0);

            // Client 2
            let client2_server_sync1 = "你好";
            // Client has not typed in document, so cursor is not present yet
            let client2_cursor_sync1_pos = None;
            // Cursor: "你好世界||"
            let client2_edit1 = "你好世界";
            let client2_cursor_edit1_pos = CursorPos::new(4, 4, 4, 4);
            let client2_edit1_c1_cursor_pos = CursorPos::new(4, 4, 4, 4);
            let client2_server_sync2 = "你好世界";
            let client2_cursor_sync2_pos = client2_cursor_edit1_pos;
            // Cursor: "|世界|"
            let client2_edit2 = "世界";
            let client2_cursor_edit2_pos = CursorPos::new(2, 2, 2, 2);

            // Server
            let server_state1 = client1_edit1;
            let server_state2 = client2_edit1;
            let server_state3 = client2_edit1;
            let server_state4 = client2_edit2;

            let client1_event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_edit1_pos,
            };

            let client1_event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_edit2_pos,
            };

            let client2_event1 = LocalMessage {
                client_id: client2_id,
                document: client2_edit1.to_string(),
                cursor_pos: client2_cursor_edit1_pos,
            };

            let client2_event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit2.to_string(),
                cursor_pos: client2_cursor_edit2_pos,
            };

            network.schedule_local_message(client1_event1, 1).unwrap();
            network.schedule_local_message(client1_event2, 5).unwrap();
            network.schedule_local_message(client2_event1, 4).unwrap();
            network.schedule_local_message(client2_event2, 8).unwrap();

            // T = 0 - Snapshot
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);

            // T = 1
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 1);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                None,
            );

            // T = 2
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 2);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                None,
            );

            // T = 3
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 3);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                client2_cursor_sync1_pos,
            );

            // T = 4
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 4);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client2_edit1_c1_cursor_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 6
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client2_cursor_edit1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 7
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 7);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                // It happens that client1 and 2 after this op will bot be at the send of string
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync2_pos),
            );

            // T = 8, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 8);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            // T = 9, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 9);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            // T = 10, only asserts state for changing components
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_multi_client_cursor_highlight_multiop_buffered() {
            // Tests multiple clients changing cursor highlights with multiple operations with buffering.
            // Tests application of other clients' operations while the client has buffered operations.
            // Time | C1                      | C2                          | Server
            //------|-------------------------|-----------------------------|------------------------
            // 0    | Snap - ""               | Snap - ""                   |
            // 1    | L1/0 - "hi |"           |                             |
            // 2    | L2   - "|hi |"          |                             | C1/S1 -> "hi |"
            // 3    | Ack(L1)
            //      | O (S1) - "|hi |"        |                             |
            // 4    |                         | L1/0 - "bye|"               | C1/S2 -> "|hi |"
            // 5    | Ack(L2)                 | L2 - "|bye"                 | C2/S3 -> "|hi |bye|"
            // 6    | S3 - "|hi |bye|"        | L3 - "good|bye"
            // 7    |                         | S1 - "hi good||bye"
            // 8    |                         | L4 - "hi goo||bye"
            // 9    |                         | S2 - "|hi goo||bye"
            // 10   |                         | Ack(L1)
            //      |                         | O (S2) - "|hi ||bye"
            // 11   |                                                        | C2/S4 -> "|hi ||bye"
            // 12   | S4 - "|hi ||bye"
            // 16   |                         | Ack(L2)
            //      |                         | O (S4) - "|hi |good|bye"     |
            // 17   |                         |                              | C2/S5 -> "|hi |good|bye"
            // 18   | S5 - "|hi good|bye|"    |                              |
            // 22   |                         | Ack(L3)                      |
            //      |                         | O (S5) - "|hi |goo|bye"      |
            // 23                                                            | C2/S6 -> "|hi |goo|bye"
            // 24   | S6 - "|hi |goo|bye"     | Ack(L4)                      |

            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(5))
                .unwrap();

            let start_text = "";

            // Client 1
            // Cursor: "hi |"
            let client1_edit1 = "hi ";
            let client1_cursor_edit1_pos = CursorPos::new(3, 3, 3, 3);
            // Cursor: "|hi |"
            let client1_edit2 = "hi ";
            let client1_cursor_edit2_pos = CursorPos::new(0, 3, 0, 3);
            // Cursor: "|hi |bye|"
            let client1_server_sync1 = "hi bye";
            let client1_cursor_sync1_pos = CursorPos::new(0, 3, 0, 3);
            // Cursor: "|hi ||bye"
            let client1_server_sync2 = "hi bye";
            let client1_cursor_sync2_pos = CursorPos::new(0, 3, 0, 3);
            let client1_sync2_c2_cursor_pos = CursorPos::new(3, 3, 3, 3);
            // Cursor: "|hi |good|bye"
            let client1_server_sync3 = "hi goodbye";
            let client1_cursor_sync3_pos = CursorPos::new(0, 3, 0, 3);

            // Client 2
            // Cursor: "bye|"
            let client2_edit1 = "bye";
            let client2_cursor_edit1_pos = CursorPos::new(3, 3, 3, 3);
            // Cursor: "|bye"
            let client2_edit2 = "bye";
            let client2_cursor_edit2_pos = CursorPos::new(0, 0, 0, 0);
            // Cursor: "good|bye"
            let client2_edit3 = "goodbye";
            let client2_cursor_edit3_pos = CursorPos::new(4, 4, 4, 4);
            // Cursor: "hi good|bye|"
            let client2_server_sync1 = "hi goodbye";
            let client2_cursor_sync1_pos = CursorPos::new(7, 7, 7, 7);
            let client2_sync1_c1_cursor_pos = CursorPos::new(10, 10, 10, 10);
            let client2_edit4 = "hi goobye";
            let client2_cursor_edit4_pos = CursorPos::new(6, 6, 6, 6);
            let client2_edit4_c1_cursor_pos = CursorPos::new(9, 9, 9, 9);
            // Cursor: "|hi goo|bye|"
            let client2_server_sync2 = "hi goobye";
            let client2_cursor_sync2_pos = CursorPos::new(6, 6, 6, 6);
            let client2_sync2_c1_cursor_pos = CursorPos::new(0, 3, 0, 3);

            // Server
            let server_state1 = client1_edit1;
            let server_state1_c1_cursor = client1_cursor_edit1_pos;
            let server_state2 = client1_edit2;
            let server_state2_c1_cursor = client1_cursor_edit2_pos;
            let server_state3 = "hi bye";
            let server_state3_c1_cursor = CursorPos::new(0, 3, 0, 3);
            let server_state3_c2_cursor = CursorPos::new(6, 6, 6, 6);
            let server_state4 = "hi bye";
            let server_state4_c1_cursor = server_state3_c1_cursor;
            let server_state4_c2_cursor = CursorPos::new(3, 3, 3, 3);
            let server_state5 = "hi goodbye";
            let server_state5_c1_cursor = CursorPos::new(0, 3, 0, 3);
            let server_state5_c2_cursor = CursorPos::new(7, 7, 7, 7);
            let server_state6 = "hi goobye";
            let server_state6_c1_cursor = CursorPos::new(0, 3, 0, 3);
            let server_state6_c2_cursor = CursorPos::new(6, 6, 6, 6);

            let client1_event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: client1_cursor_edit1_pos,
            };

            let client1_event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: client1_cursor_edit2_pos,
            };

            let client2_event1 = LocalMessage {
                client_id: client2_id,
                document: client2_edit1.to_string(),
                cursor_pos: client2_cursor_edit1_pos,
            };

            let client2_event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit2.to_string(),
                cursor_pos: client2_cursor_edit2_pos,
            };

            let client2_event3 = LocalMessage {
                client_id: client2_id,
                document: client2_edit3.to_string(),
                cursor_pos: client2_cursor_edit3_pos,
            };
            let client2_event4 = LocalMessage {
                client_id: client2_id,
                document: client2_edit4.to_string(),
                cursor_pos: client2_cursor_edit4_pos,
            };

            // Insertions
            network.schedule_local_message(client1_event1, 1).unwrap();
            network.schedule_local_message(client1_event2, 2).unwrap();
            network.schedule_local_message(client2_event1, 4).unwrap();
            network.schedule_local_message(client2_event2, 5).unwrap();
            network.schedule_local_message(client2_event3, 6).unwrap();
            // Delete
            network.schedule_local_message(client2_event4, 8).unwrap();

            // T = 0 - Snapshot
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);

            // T = 1
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 1);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit1_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 2, 3 (Ack L1)
            for t in [2, 3] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state1
                );
                assert_eq!(
                    network.component(server_id).unwrap().cursor_pos(client1_id),
                    Some(server_state1_c1_cursor)
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit2
                );
                assert_eq!(
                    network
                        .component(client1_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client1_cursor_edit2_pos)
                );
                assert_eq!(
                    network.component(client2_id).unwrap().document(),
                    start_text
                );
                assert_eq!(
                    network
                        .component(client2_id)
                        .unwrap()
                        .cursor_pos(client2_id),
                    None,
                );
            }

            // T = 4
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 4);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state2_c1_cursor)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                None
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit1_pos),
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state3_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state3_c2_cursor)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_edit2_pos)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                None
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit2_pos),
            );

            // T = 6
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state3_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state3_c2_cursor)
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync1
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync1_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                // C2 cursor on C1 screen will be the same as sent from the cursor
                Some(server_state3_c2_cursor)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit3
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                None
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit3_pos),
            );

            // T = 7
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 7);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state3_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state3_c2_cursor)
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client2_sync1_c1_cursor_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync1_pos),
            );

            // T = 8
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 8);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit4
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client2_edit4_c1_cursor_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_edit4_pos),
            );

            // T = 9, 10 (Ack C1/L2)
            for t in [9, 10] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(client2_id).unwrap().document(),
                    client2_server_sync2
                );
                assert_eq!(
                    network
                        .component(client2_id)
                        .unwrap()
                        .cursor_pos(client1_id),
                    Some(client2_sync2_c1_cursor_pos)
                );
                assert_eq!(
                    network
                        .component(client2_id)
                        .unwrap()
                        .cursor_pos(client2_id),
                    Some(client2_cursor_sync2_pos),
                );
            }

            // T = 11
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 11);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state4_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state4_c2_cursor),
            );

            // T = 12
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 12);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync2_pos)
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client1_sync2_c2_cursor_pos),
            );

            // T = 16 (C2 Ack L2)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 16);

            // T = 17
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 17);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state5
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state5_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state5_c2_cursor),
            );

            // T = 18
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 18);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync3
            );
            assert_eq!(
                network
                    .component(client1_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client1_cursor_sync3_pos)
            );

            // T = 22 (C2 Ack L3)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 22);

            // T = 23
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 23);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state6
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client1_id),
                Some(server_state6_c1_cursor)
            );
            assert_eq!(
                network.component(server_id).unwrap().cursor_pos(client2_id),
                Some(server_state6_c2_cursor),
            );

            // T = 24
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 24);
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client1_id),
                Some(client2_sync2_c1_cursor_pos)
            );
            assert_eq!(
                network
                    .component(client2_id)
                    .unwrap()
                    .cursor_pos(client2_id),
                Some(client2_cursor_sync2_pos),
            );

            assert!(matches!(network.tick(), Ok(NetworkState::Stopped)));
        }
    }

    mod text_tests {
        use super::*;

        // Tests designed primarily for text transformation tests.
        // These will still test cursor transformations as a sdie effect, but they do not explicitly assert cursor positions.
        #[test]
        fn test_network_add_components() {
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            network
                .add_component_sod(server, Delay::constant(0))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(0))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(0))
                .unwrap();

            assert_eq!(network.server_id().borrow().unwrap(), 0);
            assert_eq!(*network.client_ids().borrow(), vec![1, 2]);
            assert_eq!(network.next_id(), 3);
        }

        #[test]
        fn test_server_count() {
            // Network must have exactly one server when running
            let mut network = Network::new();
            let server1 = Box::new(ServerNetwork::new(&mut network));
            let server2 = Box::new(ServerNetwork::new(&mut network));

            network
                .add_component_sod(server1, Delay::constant(1))
                .unwrap();
            assert!(matches!(
                network.add_component_sod(server2, Delay::constant(1)),
                Err(NetworkError::DuplicateServer)
            ));

            assert!(network.run().is_ok());
        }

        #[test]
        fn test_server_single_message() {
            // Time    C1                      Server
            // 0       Snap  - ""
            // 0       Local - "hello world"
            // 1                               C1 -> "hello world"
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client = Box::new(ClientNetwork::new(&mut network));

            let client_id = client.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client, Delay::constant(1))
                .unwrap();

            // Cursor: "hello world|"
            let target_text = "hello world";
            let event = LocalMessage {
                client_id,
                document: target_text.to_string(),
                cursor_pos: CursorPos::new(
                    target_text.chars().count(),
                    target_text.chars().count(),
                    target_text.chars().count(),
                    target_text.chars().count(),
                ),
            };
            network.schedule_local_message(event, 0).unwrap();

            assert!(network.run().is_ok());
            assert_eq!(
                network.component(client_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
        }

        #[test]
        fn test_server_two_seq_message() {
            // Tests two messages occurring in sequence. No out of order transformations needed.
            //
            // Time    C1                      C2                     Server
            // 0       Snap  - ""              Snap  - ""
            // 0       L1    - "hello "
            // 1                                                      C1 -> "hello "
            // 2                               S  - "hello "
            // 3                               L1 - "hello world"
            // 4                                                      C2 -> "hello world"
            // 5       S     - "hello world"
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let start_text = "";
            let target_text = "hello world";
            let client1_edit = "hello ";
            let client2_edit = "hello world";

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                ),
            };

            let event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit.to_string(),
                cursor_pos: CursorPos::new(
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                ),
            };
            network.schedule_local_message(event1, 0).unwrap();
            network.schedule_local_message(event2, 3).unwrap();

            // T = 0
            assert!(network.tick().is_ok());
            assert!(network.time() == 0);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 1
            assert!(network.tick().is_ok());
            assert!(network.time() == 1);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 2
            assert!(network.tick().is_ok());
            assert!(network.time() == 2);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client1_edit
            );

            // T = 3
            assert!(network.tick().is_ok());
            assert!(network.time() == 3);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 4
            assert!(network.tick().is_ok());
            assert!(network.time() == 4);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client2_edit
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert!(network.time() == 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                target_text
            );
        }

        #[test]
        fn test_server_two_nonseq_message() {
            // Tests two messages occurring out of order for a client (C2), transformations needed.
            //
            // Time    C1                      C2                       Server
            // 0       Snap  - ""              Snap  - ""
            // 0       L     - "hello "
            // 1                               L     - "world"          C1 -> "hello "
            // 2                               S     - "hello world"    C2 -> "hello world"
            // 3       S     - "hello world"
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let target_text = "hello world";
            let client1_edit = "hello ";
            let client2_edit = "world";

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                    client1_edit.chars().count(),
                ),
            };

            let event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit.to_string(),
                cursor_pos: CursorPos::new(
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                ),
            };
            network.schedule_local_message(event1, 0).unwrap();
            network.schedule_local_message(event2, 1).unwrap();

            assert!(network.run().is_ok());
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                target_text
            );
        }

        #[test]
        fn test_server_two_nonseq_buffered_insert_delete() {
            // Naming explanation:
            // - Two clients
            // - Non-sequential operations: clients are concurrently updating the document
            // - Buffered insert: One set of current operations is C1 making two insertions, one of which is buffered
            // - Delete: The other concurrent operation is C2 making a deletion
            //
            // Tests:
            // - Insert and delete transformation in server
            // - C1 can make multiple local edits before receiving a server message and getting to the target state
            // - Clients do not reapply their own operations broadcast from the server
            // - C1 buffers client updates until server acks L2
            //
            // Time    | C1                                      | C2                         | Server
            // -------------------------------------------------------------------------------------------------
            // 0       | Snap  - ""                              | Snap  - ""                 |
            // 0       | L1/O  - "shello"                        |                            |
            // 5       |                                         |                            | C1 -> "shello" (S1)
            // 6       | Ack(L1)                                 | S   - "shello"     (S1)    |
            // 10      |                                         | L1/O  - "hello"            |
            // 11      | L2/O  - "shello world"      (S1)        |                            |
            // 12      | L3  - "shello worldlings" (S1)          |                            |
            // 15      |                                         |                            | C2 -> "hello" (S2)
            // 16      | S - "hello worldlings"                  | Ack(L1)                    | C1 -> "hello world" (S3)
            // 17      | Ack (L2), O  - "hello worldlings"  (S3) | S   - "hello world"        |
            // 22      |                                         |                            | C1 -> "hello worldlings" (S4)
            // 23      | Ack(L3)                                 | S   - "hello worldlings"   |
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let client1_id = client1.id();
            let client2_id = client2.id();
            let server_id = server.id();

            // Server has 5 time unit delay so C1 has time to make multiple local edits
            network
                .add_component_sod(server, Delay::constant(5))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let start_text = "";
            let target_text = "hello worldlings";
            // Cursor: "shello|"
            let client1_edit1 = "shello";
            // Cursor: "shello world|"
            let client1_edit2 = "shello world";
            // Cursor: "shello worldlings|"
            let client1_edit3 = "shello worldlings";
            // Cursor: "hello|"
            let client2_edit = "hello";
            let server_state1 = client1_edit1;
            let server_state2 = client2_edit;
            let server_state3 = "hello world";

            let event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                ),
            };

            let event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit.to_string(),
                cursor_pos: CursorPos::new(
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                    client2_edit.chars().count(),
                ),
            };

            let event3 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                ),
            };

            let event4 = LocalMessage {
                client_id: client1_id,
                document: client1_edit3.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                ),
            };
            network.schedule_local_message(event1, 0).unwrap();
            network.schedule_local_message(event2, 10).unwrap();
            network.schedule_local_message(event3, 11).unwrap();
            network.schedule_local_message(event4, 12).unwrap();

            // T = 0
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 6
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client1_edit1
            );

            // T = 10
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 11
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 11);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 12
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 12);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 15
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 15);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 16
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 16);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit
            );

            // T = 17
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 17);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                server_state3
            );

            // T = 22
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 22);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                server_state3
            );

            // T = 23
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 23);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                target_text
            );

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_server_two_nonseq_buffered_multi_ops() {
            // Naming explanation:
            // - Two clients
            // - Non-sequential operations: clients are concurrently updating the document
            // - Buffered: Both clients will be bufferring messages of up to length 3
            // - Multi-ops: Buffered messages from both clients will be of either insert or delete
            //
            // Adapted from Daniel Spiewak's final complex OT example in:
            // https://web.archive.org/web/20120107060932/http://www.codecommit.com/blog/java/understanding-and-applying-operational-transformation
            //
            // Time    | C1                                            | C2                                       | Server
            // ------------------------------------------------------  -------------------------------------------------------------------------------------------
            // 0       | L1/O (S0) - "Nice"                            | L1/O -"Hi,"                              |
            // 1       |                                               |                                          | C2 -> "Hi," (S1)
            // 2       |                                               | Ack(L1)                                  |
            // 5       | L2   - "Nice to meet"                         |                                          |
            // 6       | S  - "Hi,Nice to meet"                        |                                          |
            // 7       | L3 - "Hi! Nice to meet you."                  |                                          |
            // 8       |                                               | L2/O -"Hi, how are you? "                |
            // 9       |                                               |                                          | C2 -> "Hi, how are you?" (S2)
            // 10      |                                               | Ack(L2)                                  | C1 -> "Hi, how are you?Nice" (S3)
            // 11      |                                               | S - "Hi, how are you? Nice"              |
            // 14      | S - "Hi! how are you? Nice to meet you."      |                                          |
            // 15      | Ack(L1),                                      |                                          |
            //         | O (S3) - "Hi, how are you?Nice to meet"       |                                          |
            // 25      |                                               |                                          | C1 -> "Hi, how are you?Nice to meet" (S4)
            // 26      |                                               | S - "Hi, how are you?Nice to meet"       |
            // 30      | Ack(L2),                                      |                                          |
            //         | O (S4) - "Hi how are you?! Nice to meet you." |                                          |
            // 40      |                                               |                                          | C1 -> "Hi how are you?! Nice to meet you." (S5)
            // 41      |                                               | S - "Hi how are you?! Nice to meet you." |
            // 45      | Ack(L3)                                       |                                          |

            let mut network = Network::new();

            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));
            let client2 = Box::new(ClientNetwork::new(&mut network));

            let server_id = server.id();
            let client1_id = client1.id();
            let client2_id = client2.id();

            network
                .add_component_sod(
                    server,
                    Delay::variable(vec![(client1_id, 10), (client2_id, 1)], 10),
                )
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(5))
                .unwrap();
            network
                .add_component_sod(client2, Delay::constant(1))
                .unwrap();

            let start_text = "";
            // Cursor: "Nice|"
            let client1_edit1 = "Nice";
            // Cursor: "Nice to meet|"
            let client1_edit2 = "Nice to meet";
            // Client replaces the `,` with `!` from a synced Client 2 edit. Collaboration!
            // Also adds `you.` to the end of the string.
            // Cursor: "Hi! Nice to meet you.|"
            let client1_edit3 = "Hi! Nice to meet you.";
            // Client2 inserts "Hi," first without a tailing space
            // Cursor: "Hi,Nice to meet|"
            let client1_server_sync1 = "Hi,Nice to meet";
            // While this string may seem odd ("Hi! how are you? Nice to meet you." is intuitively expected instead),
            // this is the expected result of applying the server insert (" how are you?") prior to the client's (insert "!")
            // both starting at index 2 (after "Hi").
            // Cursor: "Hi how are you?! Nice to meet you.|"
            let client1_server_sync2 = "Hi how are you?! Nice to meet you.";
            // Cursor: "Hi,|"
            let client2_edit1 = "Hi,";
            // Cursor: "Hi, how are you?|"
            let client2_edit2 = "Hi, how are you?";
            // Client 1's update to correct the missing space is still in flight for some time
            let client2_server_sync1 = "Hi, how are you?Nice";
            let client2_server_sync2 = "Hi, how are you?Nice to meet";
            // Client 1's update to correct the missing space is received
            let client2_server_sync3 = "Hi how are you?! Nice to meet you.";
            let server_state1 = "Hi,";
            let server_state2 = "Hi, how are you?";
            // Client 1's update to correct the missing space is still in flight for some time
            let server_state3 = "Hi, how are you?Nice";
            let server_state4 = "Hi, how are you?Nice to meet";
            // Client 1's update to correct the missing space is received
            let server_state5 = "Hi how are you?! Nice to meet you.";
            let target_text = "Hi how are you?! Nice to meet you.";

            let client1_event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                ),
            };

            let client1_event2 = LocalMessage {
                client_id: client1_id,
                document: client1_edit2.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                    client1_edit2.chars().count(),
                ),
            };

            let client1_event3 = LocalMessage {
                client_id: client1_id,
                document: client1_edit3.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                    client1_edit3.chars().count(),
                ),
            };

            let client2_event1 = LocalMessage {
                client_id: client2_id,
                document: client2_edit1.to_string(),
                cursor_pos: CursorPos::new(
                    client2_edit1.chars().count(),
                    client2_edit1.chars().count(),
                    client2_edit1.chars().count(),
                    client2_edit1.chars().count(),
                ),
            };

            let client2_event2 = LocalMessage {
                client_id: client2_id,
                document: client2_edit2.to_string(),
                cursor_pos: CursorPos::new(
                    client2_edit2.chars().count(),
                    client2_edit2.chars().count(),
                    client2_edit2.chars().count(),
                    client2_edit2.chars().count(),
                ),
            };

            network.schedule_local_message(client1_event1, 0).unwrap();
            network.schedule_local_message(client1_event2, 5).unwrap();
            network.schedule_local_message(client1_event3, 7).unwrap();
            network.schedule_local_message(client2_event1, 0).unwrap();
            network.schedule_local_message(client2_event2, 8).unwrap();

            // T = 0
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );

            // T = 1, T = 2 (C2 acks L1, no-op)
            for t in [1, 2] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    server_state1
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    client1_edit1
                );
                assert_eq!(
                    network.component(client2_id).unwrap().document(),
                    client2_edit1
                );
            }

            // T = 5
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );

            // T = 6
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync1
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );

            // T = 7
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 7);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit1
            );

            // T = 8
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 8);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state1
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );

            // T = 9
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 9);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state2
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );

            // T = 10
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 10);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_edit2
            );

            // T = 11
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 11);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit3
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );

            // T = 14
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 14);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );

            // T = 15 (C1 acks L1)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 15);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state3
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );

            // T = 25
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 25);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync1
            );

            // T = 26
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 26);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );

            // T = 30 (C1 acks L2)
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 30);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state4
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );

            // T = 40
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 40);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state5
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync2
            );

            // T = 41
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 41);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                server_state5
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_server_sync2
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                client2_server_sync3
            );

            // T = 45
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 45);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                target_text
            );

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }

        #[test]
        fn test_late_joiner_sync() {
            let mut network = Network::new();
            let server = Box::new(ServerNetwork::new(&mut network));
            let client1 = Box::new(ClientNetwork::new(&mut network));

            let client2_id = 2;
            let client1_id = client1.id();
            let server_id = server.id();

            network
                .add_component_sod(server, Delay::constant(1))
                .unwrap();
            network
                .add_component_sod(client1, Delay::constant(1))
                .unwrap();

            let start_text = "";
            // Cursor: "Client 1 is typing!|"
            let client1_edit1 = "Client 1 is typing!";
            let target_text = client1_edit1;

            let client1_event1 = LocalMessage {
                client_id: client1_id,
                document: client1_edit1.to_string(),
                cursor_pos: CursorPos::new(
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                    client1_edit1.chars().count(),
                ),
            };

            network.schedule_local_message(client1_event1, 0).unwrap();
            network.schedule_late_joiner(
                5,
                ClientConfig {
                    delay: Delay::constant(1),
                },
            );

            // T = 0, client2 does not yet exist
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 0);
            assert_eq!(network.component(server_id).unwrap().document(), start_text);
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                client1_edit1
            );
            assert!(network.component(client2_id).is_none());

            // T = 1, T = 2 (C1 acks L1, no-op)
            for t in [1, 2] {
                assert!(network.tick().is_ok());
                assert_eq!(network.time(), t);
                assert_eq!(
                    network.component(server_id).unwrap().document(),
                    target_text
                );
                assert_eq!(
                    network.component(client1_id).unwrap().document(),
                    target_text
                );
                assert!(network.component(client2_id).is_none());
            }

            // T = 5, client2 is created but the document is empty
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 5);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                start_text
            );

            // T = 6, client2 receives the server state
            assert!(network.tick().is_ok());
            assert_eq!(network.time(), 6);
            assert_eq!(
                network.component(server_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client1_id).unwrap().document(),
                target_text
            );
            assert_eq!(
                network.component(client2_id).unwrap().document(),
                target_text
            );

            assert!(network.tick().unwrap() == NetworkState::Stopped);
        }
    }
}
