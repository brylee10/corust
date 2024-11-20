use std::marker::PhantomData;

use corust_types::{ContainerMessage, ContainerResponse};
use serde::{de::DeserializeOwned, Serialize};
use tokio_util::{
    bytes::{Buf, BufMut, BytesMut},
    codec::{Decoder, Encoder},
};

use crate::MESSAGE_BUF_SIZE_BYTES;

/// Generic encoder and decoder for length prefixed binary encodings.
pub struct ContainerCodec<T> {
    _marker: PhantomData<T>,
}

impl<T> ContainerCodec<T> {
    pub fn new() -> Self {
        ContainerCodec {
            _marker: std::marker::PhantomData,
        }
    }
}

/// Encodes and decodes messages from a [`ContainerMessage`] to and from a byte stream
pub type ContainerMessageCodec = ContainerCodec<ContainerMessage>;
/// Encodes and decodes messages from a [`ContainerResponse`] to aand from a byte stream
pub type ContainerResponseCodec = ContainerCodec<ContainerResponse>;

impl<T> Encoder<T> for ContainerCodec<T>
where
    T: Serialize,
{
    type Error = bincode::Error;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let message_size = bincode::serialized_size(&item)?;
        log::debug!(
            "Serializing a ContainerMessage with size bytes: {}",
            message_size
        );
        let message = bincode::serialize(&item)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        dst.reserve(MESSAGE_BUF_SIZE_BYTES + message.len());

        dst.put_u64_le(message_size);
        dst.extend_from_slice(&message);
        Ok(())
    }
}

impl<T> Decoder for ContainerCodec<T>
where
    T: DeserializeOwned,
{
    type Item = T;
    type Error = bincode::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < MESSAGE_BUF_SIZE_BYTES {
            // Not enough bytes, Poll::Pending
            return Ok(None);
        }

        // Peek at the message size without consuming bytes
        let message_size = {
            let size_bytes = &src[..MESSAGE_BUF_SIZE_BYTES];
            u64::from_le_bytes(size_bytes.try_into().unwrap()) as usize
        };

        if src.len() < MESSAGE_BUF_SIZE_BYTES + message_size {
            // Not enough bytes, Poll::Pending
            return Ok(None);
        }

        src.advance(MESSAGE_BUF_SIZE_BYTES);
        let message = src.split_to(message_size);
        let item = bincode::deserialize(&message)?;
        Ok(Some(item))
    }
}
