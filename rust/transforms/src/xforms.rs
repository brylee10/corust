//! Operational Transformations
//!
//! These transformation functions make no assumption about "happens before" relations.
//! The functions define `f(op1, op2) -> (op1', op2')` where `op1'` and `op2'` are the transformed operations.
//! The operations have the property that `op1 * op2' = op2 * op1'` where `*` is the composition operator and `=`
//! means the resulting output when applied to an equivalent original document is the same (for text output, this
//! means the hash of the text is the same).
//!
//! Inspired by Google Wave's implementation of Operational Transform:
//! https://svn.apache.org/repos/asf/incubator/wave/whitepapers/operational-transform/operational-transform.html

use std::cmp::Ordering;

use crate::ops::CompoundOp;
#[cfg(feature = "js")]
use crate::web_utils::debug;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use wasm_bindgen::prelude::*;

// JS error logging
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

/// Error type for text operation errors
#[derive(Error, Debug)]
pub enum TextOperationError {
    #[error("Input length is incorrect. Expect {expected} chars, got {actual} chars")]
    IncorrectInputLength { expected: usize, actual: usize },
    #[error(
        "Composition is incompatible. Expected input size {expected_input}, got {actual_input}"
    )]
    CompositionIncompatible {
        expected_input: usize,
        actual_input: usize,
    },
}

/// The `TextOperation` struct represents a sequence of operations
// This is similar to a CodeMirror Transaction.
#[wasm_bindgen]
#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct TextOperation {
    // Applied in order, first to last
    ops: Vec<CompoundOp>,
    // The length of the document before the operation was applied
    // The length is the number of characters in the document, not the number
    // of UTF-8 bytes.
    input_length: usize,
    // The length of the document after the operation was applied
    // The length is the number of characters in the document, not the number
    // of UTF-8 bytes.
    output_length: usize,
    // Randomly generated id uniquely identifying this operation
    id: Uuid,
    // Flag indicating if operation is a transformation or a generated operation. Useful for sanity checks.
    is_transform: bool,
}

/// Default `TextOperation` is equivalent to a no-op on an empty document.
impl Default for TextOperation {
    fn default() -> Self {
        Self {
            ops: vec![],
            input_length: 0,
            output_length: 0,
            id: Uuid::new_v4(),
            is_transform: false,
        }
    }
}

#[wasm_bindgen]
impl TextOperation {
    pub fn to_json_string(&self) -> Result<String, JsValue> {
        serde_json::to_string(self).map_err(|e| {
            let error_message = format!("Serialization error: {}", e);
            error(&error_message);
            JsValue::from_str(&error_message)
        })
    }

    // JS friendly serialization
    pub fn serialize_text_operation_js(&self) -> Result<JsValue, JsValue> {
        match self.to_json_string() {
            Ok(json_str) => Ok(JsValue::from_str(&json_str)),
            Err(_) => {
                let error_message = "Error serializing TextOperation".to_string();
                error(&error_message);
                Err(JsValue::from_str(&error_message))
            }
        }
    }

    pub fn from_json_string(s: &str) -> Result<TextOperation, JsValue> {
        serde_json::from_str(s).map_err(|e| {
            let error_message = format!("Deserialization error: {}", e);
            error(&error_message);
            JsValue::from_str(&error_message)
        })
    }

    pub fn id_str(&self) -> String {
        self.id.to_string()
    }
}
impl TextOperation {
    /// Converts a series of `CompoundOp` operations into a `TextOperation` struct. If `id` is provided, it is used as the id of the operation.
    /// Otherwise, a new ID is generated. `is_transform` is set by derived operations from the `transform` function.
    pub fn from_ops<T: Iterator<Item = CompoundOp>>(
        ops: T,
        id: Option<Uuid>,
        is_transform: bool,
    ) -> Self {
        // Performance Hit: This is effectively cloning the input data structure.
        let ops: Vec<_> = ops.collect();
        // Infers `input_length` from the series of operations.
        let input_length = ops.iter().fold(0, |acc, op| op.expected_input_size() + acc);

        let total_len: isize = ops.iter().fold(0, |acc, op| op.op_output_len() + acc);

        // Deletes can reduce the length of the document. However, the number of deletes cannot execede the `input_length`, so
        // total output length should never be negative.
        let output_length = usize::try_from(total_len).unwrap();
        let id = id.unwrap_or_else(Uuid::new_v4);
        Self {
            ops,
            input_length,
            output_length,
            id,
            is_transform,
        }
    }

    // Creates a new [`TextOperation`] where all consecutive operations of the same type are merged into one operation.
    fn condense(self) -> Self {
        let mut condensed_ops = vec![];
        let mut next_ops = self.ops.iter().peekable();
        while let Some(op) = next_ops.next() {
            match op {
                CompoundOp::Retain { count: c1 } => {
                    let mut count = *c1;
                    while let Some(CompoundOp::Retain { count: c2 }) = next_ops.peek() {
                        count += c2;
                        next_ops.next();
                    }
                    condensed_ops.push(CompoundOp::Retain { count });
                }
                CompoundOp::Insert { text: s1 } => {
                    let mut text = s1.clone();
                    while let Some(CompoundOp::Insert { text: s2 }) = next_ops.peek() {
                        text.push_str(s2);
                        next_ops.next();
                    }
                    condensed_ops.push(CompoundOp::Insert { text });
                }
                CompoundOp::Delete { count: c1 } => {
                    let mut count = *c1;
                    while let Some(CompoundOp::Delete { count: c2 }) = next_ops.peek() {
                        count += c2;
                        next_ops.next();
                    }
                    condensed_ops.push(CompoundOp::Delete { count });
                }
            }
        }
        Self::from_ops(condensed_ops.into_iter(), Some(self.id), self.is_transform)
    }

    /// OperationalTransformation
    ///
    /// Given transformation `T`, `T(ops1, ops2) -> (ops1', ops2')` and `apply(apply(S, ops1), ops2') = apply(apply(S, ops2), ops1')`
    /// where `S` is the original document, `ops1` and `ops2` are the operations to be transformed, and `ops1'` and `ops2'` are the transformed operations.
    ///
    /// Note that there are many possible `ops1'` and `ops2'` that satisfy the above equation. For example, ops1' = ops2' = "delete everything" would suffice, but
    /// this would not be expected for users. This function returns one of the possible transformations. `op1` represents the "server" current state and `op2` represents
    /// an incoming user request. The transformation prioritzes "op1" in conflicts, i.e. both `op1` and `op2` will be applied but `op1` will be applied before `op2`, i.e.
    /// `op1` "happens before" `op2`.  
    ///
    /// `transform` maps the id of the original two operations to the transformed operations so the same operation (transformed or not) is not reapplied by clients
    /// when broadcast from a server.
    pub fn transform(&self, other: &Self) -> Result<(Self, Self), TextOperationError> {
        if self.input_length != other.input_length {
            #[cfg(feature = "js")]
            debug(&format!(
                "self: {}, other: {}",
                serde_json::to_string(&self).unwrap(),
                serde_json::to_string(&other).unwrap()
            ));
            return Err(TextOperationError::IncorrectInputLength {
                expected: self.input_length,
                actual: other.input_length,
            });
        }

        let mut ops1_prime = vec![];
        let mut ops2_prime = vec![];

        let mut next_ops1 = self.ops.iter().cloned();
        let mut next_ops2 = other.ops.iter().cloned();
        let mut op1 = next_ops1.next();
        let mut op2 = next_ops2.next();

        while op1.is_some() || op2.is_some() {
            match (op1.as_ref(), op2.as_ref()) {
                (
                    Some(&CompoundOp::Retain { count: c1 }),
                    Some(&CompoundOp::Retain { count: c2 }),
                ) => {
                    let min_count = std::cmp::min(c1, c2);
                    ops1_prime.push(CompoundOp::Retain { count: min_count });
                    ops2_prime.push(CompoundOp::Retain { count: min_count });

                    match c1.cmp(&c2) {
                        Ordering::Greater => {
                            op1 = Some(CompoundOp::Retain { count: c1 - c2 });
                            op2 = next_ops2.next();
                        }
                        Ordering::Less => {
                            op1 = next_ops1.next();
                            op2 = Some(CompoundOp::Retain { count: c2 - c1 });
                        }
                        Ordering::Equal => {
                            op1 = next_ops1.next();
                            op2 = next_ops2.next();
                        }
                    }
                }
                (
                    Some(&CompoundOp::Delete { count: c1 }),
                    Some(&CompoundOp::Delete { count: c2 }),
                ) => match c1.cmp(&c2) {
                    Ordering::Greater => {
                        op1 = Some(CompoundOp::Delete { count: c1 - c2 });
                        op2 = next_ops2.next();
                    }
                    Ordering::Less => {
                        op1 = next_ops1.next();
                        op2 = Some(CompoundOp::Delete { count: c2 - c1 });
                    }
                    Ordering::Equal => {
                        op1 = next_ops1.next();
                        op2 = next_ops2.next();
                    }
                },
                (
                    Some(&CompoundOp::Delete { count: c1 }),
                    Some(&CompoundOp::Retain { count: c2 }),
                ) => {
                    let min_count = std::cmp::min(c1, c2);
                    ops1_prime.push(CompoundOp::Delete { count: min_count });
                    match c1.cmp(&c2) {
                        Ordering::Greater => {
                            op1 = Some(CompoundOp::Delete { count: c1 - c2 });
                            op2 = next_ops2.next();
                        }
                        Ordering::Less => {
                            op1 = next_ops1.next();
                            op2 = Some(CompoundOp::Retain { count: c2 - c1 });
                        }
                        Ordering::Equal => {
                            op1 = next_ops1.next();
                            op2 = next_ops2.next();
                        }
                    }
                }
                (
                    Some(&CompoundOp::Retain { count: c1 }),
                    Some(&CompoundOp::Delete { count: c2 }),
                ) => {
                    let min_count = std::cmp::min(c1, c2);
                    ops2_prime.push(CompoundOp::Delete { count: min_count });

                    match c1.cmp(&c2) {
                        Ordering::Greater => {
                            op1 = Some(CompoundOp::Retain { count: c1 - c2 });
                            op2 = next_ops2.next();
                        }
                        Ordering::Less => {
                            op1 = next_ops1.next();
                            op2 = Some(CompoundOp::Delete { count: c2 - c1 });
                        }
                        Ordering::Equal => {
                            op1 = next_ops1.next();
                            op2 = next_ops2.next();
                        }
                    }
                }
                // `op1` insertions "happens before" `op2` insertions
                (Some(CompoundOp::Insert { text: s }), _) => {
                    ops1_prime.push(CompoundOp::Insert { text: s.clone() });
                    ops2_prime.push(CompoundOp::Retain {
                        count: s.chars().count(),
                    });
                    op1 = next_ops1.next();
                }
                (_, Some(CompoundOp::Insert { text: s })) => {
                    ops2_prime.push(CompoundOp::Insert { text: s.clone() });
                    ops1_prime.push(CompoundOp::Retain {
                        count: s.chars().count(),
                    });
                    op2 = next_ops2.next();
                }
                // Unreachable state
                (_, None) | (None, _) => {
                    debug_assert!(false, "This state should never be reachable. Incorrect input length is checked on function entry.");
                    return Err(TextOperationError::IncorrectInputLength {
                        expected: self.input_length,
                        actual: other.input_length,
                    });
                }
            }
        }

        let ops1_prime =
            TextOperation::from_ops(ops1_prime.into_iter(), Some(self.id), true).condense();
        let ops2_prime =
            TextOperation::from_ops(ops2_prime.into_iter(), Some(other.id), true).condense();
        // Input lengths will differ, but output will converge
        debug_assert!(ops1_prime.output_length == ops2_prime.output_length);
        Ok((ops1_prime, ops2_prime))
    }

    /// Composes two operations into one equivalent operation.
    /// Properties are as described by Google Wave section "Composition":
    /// https://svn.apache.org/repos/asf/incubator/wave/whitepapers/operational-transform/operational-transform.html
    ///
    /// Given transformations `A`, `B`, document `d` and composition `C` where `C = A * B` (`A` applied before `B`), then
    /// `C(d) = B(A(d))`. Here `self` is A and `other` is B.
    pub fn compose(&self, other: &Self) -> Result<Self, TextOperationError> {
        if self.output_length != other.input_length {
            return Err(TextOperationError::CompositionIncompatible {
                expected_input: self.output_length,
                actual_input: other.input_length,
            });
        }

        let mut a_ops = self.ops.iter().cloned();
        let mut b_ops = other.ops.iter().cloned();
        let mut next_a = a_ops.next();
        let mut next_b = b_ops.next();

        let mut composed_ops = vec![];

        loop {
            match (&next_a, &next_b) {
                (Some(CompoundOp::Retain { count: a }), Some(CompoundOp::Retain { count: b })) => {
                    let min_count = std::cmp::min(*a, *b);
                    if min_count > 0 {
                        composed_ops.push(CompoundOp::Retain { count: min_count });
                    }
                    match a.cmp(b) {
                        Ordering::Greater => {
                            next_a = Some(CompoundOp::Retain { count: *a - *b });
                            next_b = b_ops.next();
                        }
                        Ordering::Less => {
                            next_b = Some(CompoundOp::Retain { count: *b - *a });
                            next_a = a_ops.next();
                        }
                        Ordering::Equal => {
                            next_a = a_ops.next();
                            next_b = b_ops.next();
                        }
                    }
                }
                (Some(CompoundOp::Retain { count: a }), Some(CompoundOp::Delete { count: b })) => {
                    let min_count = std::cmp::min(*a, *b);
                    if min_count > 0 {
                        composed_ops.push(CompoundOp::Delete { count: min_count });
                    }
                    match a.cmp(b) {
                        Ordering::Greater => {
                            next_a = Some(CompoundOp::Retain { count: *a - *b });
                            next_b = b_ops.next();
                        }
                        Ordering::Less => {
                            next_b = Some(CompoundOp::Delete { count: *b - *a });
                            next_a = a_ops.next();
                        }
                        Ordering::Equal => {
                            next_a = a_ops.next();
                            next_b = b_ops.next();
                        }
                    };
                }
                (Some(CompoundOp::Delete { count: a }), _) => {
                    composed_ops.push(CompoundOp::Delete { count: *a });
                    next_a = a_ops.next();
                }
                (Some(CompoundOp::Insert { text: s }), Some(CompoundOp::Delete { count: b })) => {
                    // Delete over an insert equals no-op for overlapping section
                    let num_chars = s.chars().count();
                    let min_count = std::cmp::min(num_chars, *b);
                    match num_chars.cmp(b) {
                        Ordering::Greater => {
                            // Get the end index of the `min_count`th character
                            let byte_index = s
                                .char_indices()
                                .nth(min_count)
                                .map(|(i, _)| i)
                                .unwrap_or(s.len());
                            next_a = Some(CompoundOp::Insert {
                                text: s[byte_index..].to_string(),
                            });
                            next_b = b_ops.next();
                        }
                        Ordering::Less => {
                            next_b = Some(CompoundOp::Delete {
                                count: *b - num_chars,
                            });
                            next_a = a_ops.next();
                        }
                        Ordering::Equal => {
                            next_a = a_ops.next();
                            next_b = b_ops.next();
                        }
                    };
                }
                (Some(CompoundOp::Insert { text: s }), Some(CompoundOp::Retain { count: b })) => {
                    let num_chars = s.chars().count();
                    let min_count = std::cmp::min(num_chars, *b);
                    // Get the end index of the `min_count`th character
                    let byte_index = s
                        .char_indices()
                        .nth(min_count)
                        .map(|i| i.0)
                        .unwrap_or(s.len());
                    if min_count > 0 {
                        composed_ops.push(CompoundOp::Insert {
                            text: s[..byte_index].to_string(),
                        });
                    }

                    match num_chars.cmp(b) {
                        Ordering::Greater => {
                            next_a = Some(CompoundOp::Insert {
                                text: s[byte_index..].to_string(),
                            });
                            next_b = b_ops.next();
                        }
                        Ordering::Less => {
                            next_b = Some(CompoundOp::Delete {
                                count: *b - num_chars,
                            });
                            next_a = a_ops.next();
                        }
                        Ordering::Equal => {
                            next_a = a_ops.next();
                            next_b = b_ops.next();
                        }
                    };
                }
                (_, Some(CompoundOp::Insert { text: s })) => {
                    // Since op A is applied first, op B is assumed to know the
                    // text after op A. This means if both op B and op A insert
                    // at the same position, the intent is op B to insert text
                    // in front of op A's inserted text. So Op B inserts are
                    // composed first.
                    // e.g. Op A - Insert "World" at 0
                    //      Op B - Insert "Hello " at 0
                    // Composed is Insert "Hello World" at 0
                    composed_ops.push(CompoundOp::Insert {
                        text: s.to_string(),
                    });
                    next_b = b_ops.next();
                }
                (None, Some(op)) => {
                    composed_ops.push(op.clone());
                    next_b = b_ops.next();
                }
                (Some(op), None) => {
                    composed_ops.push(op.clone());
                    next_a = a_ops.next();
                }
                (None, None) => break,
            }
        }
        // The composed op inherits no ID and is not a transformation
        let text_op = TextOperation::from_ops(composed_ops.into_iter(), None, false);
        Ok(text_op)
    }

    /// Applies the list of operations in order to `input_text`
    ///
    /// Function can be expressed as `apply(input, ops) -> output` where `input` is the input text, `ops` is the list of operations
    pub fn apply(&self, input_text: &str) -> Result<String, TextOperationError> {
        if input_text.chars().count() != self.input_length {
            log::debug!("Input text: {}", input_text);
            return Err(TextOperationError::IncorrectInputLength {
                expected: self.input_length,
                actual: input_text.chars().count(),
            });
        }

        let mut output_text = "".to_string();
        let mut input_chars = input_text.chars();
        for op in &self.ops {
            match op {
                CompoundOp::Retain { count: n } => {
                    output_text.extend(input_chars.by_ref().take(*n))
                }
                CompoundOp::Insert { text: s } => {
                    output_text.push_str(s);
                }
                CompoundOp::Delete { count: n } => {
                    if *n > 0 {
                        input_chars.by_ref().nth(*n - 1);
                    }
                }
            }
        }
        Ok(output_text)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn is_transform(&self) -> bool {
        self.is_transform
    }

    pub fn ops(&self) -> &[CompoundOp] {
        &self.ops
    }

    pub fn input_length(&self) -> usize {
        self.input_length
    }

    pub fn output_length(&self) -> usize {
        self.output_length
    }

    /// Identifies a special case where the `TextOperation` is a no-op, i.e. it only retains the input text.
    pub fn noop(&self) -> bool {
        self.ops
            .iter()
            .all(|op| matches!(op, CompoundOp::Retain { .. }))
    }
}

/// Represents a single text update, typically an input from a text editor.
/// Text range `prev` is replaced with `text` in range `next`.
/// A series of `TextUpdate` operations can be converted into a `TextOperation` struct.
/// Closely resembles the CodeMirror `ChangeSet` representation, but is generalizable.
#[wasm_bindgen]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextUpdate {
    prev: TextUpdateRange,
    next: TextUpdateRange,
    text: String,
}

#[wasm_bindgen]
impl TextUpdate {
    pub fn new(prev: TextUpdateRange, next: TextUpdateRange, text: String) -> Self {
        Self { prev, next, text }
    }

    #[inline]
    pub fn prev(&self) -> TextUpdateRange {
        self.prev
    }

    #[inline]
    pub fn next(&self) -> TextUpdateRange {
        self.next
    }

    #[inline]
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

/// A text update range `[from, to]`. Indices are in characters, not bytes.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextUpdateRange {
    from: usize,
    to: usize,
}

#[wasm_bindgen]
impl TextUpdateRange {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }

    #[inline]
    pub fn from(&self) -> usize {
        self.from
    }

    #[inline]
    pub fn to(&self) -> usize {
        self.to
    }

    // Length is in characters, not bytes
    #[inline]
    pub fn len(&self) -> usize {
        self.to - self.from
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Converts a series of `TextUpdate` operations into a `TextOperation` struct.
/// `TextUpdate`s only reflect changed document portions. Intermediate retain operations
/// are inferred.
pub fn text_updates_to_text_operation(
    updates: &[TextUpdate],
    prev_doc_len: usize,
) -> TextOperation {
    let mut ops = vec![];
    let mut input_length = 0;
    for update in updates {
        let prev_len = update.prev.len();
        let next_len = update.next.len();
        let retain_len = update.prev.from - input_length;
        if retain_len > 0 {
            ops.push(CompoundOp::Retain { count: retain_len });
        }
        if next_len > 0 {
            ops.push(CompoundOp::Insert {
                text: update.text.clone(),
            });
        }
        if prev_len > 0 {
            ops.push(CompoundOp::Delete { count: prev_len });
        }
        input_length = update.prev.to;
    }
    let retain_len = prev_doc_len - input_length;
    if retain_len > 0 {
        ops.push(CompoundOp::Retain { count: retain_len });
    }
    TextOperation::from_ops(ops.into_iter(), None, false)
}

/// Converts a [`TextOperation`] into a series of [`TextUpdate`] operations.
pub fn text_operation_text_updates(text_op: &TextOperation) -> Vec<TextUpdate> {
    let mut updates = vec![];
    // Index into the input string
    let mut input_idx = 0;
    // Index into the output string
    let mut output_idx = 0;
    for op in text_op.ops() {
        match op {
            CompoundOp::Retain { count: n } => {
                // `TextUpdate` only holds updated text, not retains
                input_idx += n;
                output_idx += n;
            }
            CompoundOp::Insert { text: s } => {
                updates.push(TextUpdate::new(
                    TextUpdateRange::new(input_idx, input_idx),
                    TextUpdateRange::new(output_idx, output_idx + s.chars().count()),
                    s.clone(),
                ));
                output_idx += s.chars().count();
            }
            CompoundOp::Delete { count: n } => {
                updates.push(TextUpdate::new(
                    TextUpdateRange::new(input_idx, input_idx + n),
                    TextUpdateRange::new(output_idx, output_idx),
                    "".to_string(),
                ));
                input_idx += n;
            }
        }
    }

    // After all ops are converted, the input and output indices should be at the end of the
    // input and output strings respectively.
    debug_assert!(input_idx == text_op.input_length);
    debug_assert!(output_idx == text_op.output_length);

    updates
}

/// Helper to create a [`TextUpdate`] for the trivial case where an entire doc is
/// inserted. The document length is counted in characters, not bytes. This matches
/// how the front end interface handles text.
pub fn text_update_from_doc(doc: &str) -> Vec<TextUpdate> {
    vec![TextUpdate::new(
        TextUpdateRange::new(0, 0),
        TextUpdateRange::new(0, doc.chars().count()),
        doc.to_string(),
    )]
}

#[cfg(test)]
mod test {
    use super::*;

    mod text_operations {
        use super::*;
        #[test]
        fn test_apply_delete() {
            let ops = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Delete { count: 2 },
                // Add another retain, otherwise prior operation could be a no-op instead of advancing the underlying iterator, giving `pan` as output
                CompoundOp::Retain { count: 1 },
            ];
            let input_text = "panda";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text).unwrap();
            assert_eq!(output_text, "paa");
        }

        #[test]
        fn test_apply_insert() {
            let ops = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Insert {
                    text: "nda".to_string(),
                },
                CompoundOp::Retain { count: 1 },
            ];
            let input_text = "pas";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text).unwrap();
            assert_eq!(output_text, "pandas");
        }

        #[test]
        fn test_apply_sequence_1() {
            // Test applying a interleaved sequence of operators
            let ops = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Insert {
                    text: "nda".to_string(),
                },
                CompoundOp::Retain { count: 6 },
                CompoundOp::Delete { count: 7 },
                CompoundOp::Insert {
                    text: "very".to_string(),
                },
                CompoundOp::Retain { count: 5 },
            ];
            let input_text = "pas are sort of cute";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text).unwrap();
            assert_eq!(output_text, "pandas are very cute");
        }

        // Test the `TextOperation` struct accurately infers the input length its operations are applied to.
        #[test]
        fn test_input_output_length_inference() {
            // Possible input is `hello` -> `hel world`
            let ops = vec![
                CompoundOp::Retain { count: 3 },
                CompoundOp::Delete { count: 2 },
                CompoundOp::Insert {
                    text: "world".to_string(),
                },
            ];
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            assert_eq!(text_op.input_length, 5);
            assert_eq!(text_op.output_length, 8);
        }

        #[test]
        fn test_text_operation_apply() {
            let ops = vec![
                CompoundOp::Retain { count: 3 },
                CompoundOp::Insert {
                    text: "world".to_string(),
                },
                CompoundOp::Delete { count: 2 },
            ];
            let input_text = "hello";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text).unwrap();
            assert_eq!(output_text, "helworld");
        }

        #[test]
        fn test_text_operation_apply_multilingual() {
            // Tests the `TextOperation` struct can handle multilingual text, particularly
            // where one char is multiple bytes in UTF-8.
            let ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 6 },
                CompoundOp::Insert {
                    text: ", 你好🌍".to_string(),
                },
            ];
            let input_text = "hello world";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text).unwrap();
            assert_eq!(output_text, "hello, 你好🌍");
        }

        #[test]
        fn test_incorrect_input_length() {
            let ops = vec![
                CompoundOp::Retain { count: 3 },
                CompoundOp::Delete { count: 2 },
            ];
            let input_text = "too long";
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let output_text = text_op.apply(input_text);

            assert!(matches!(
                output_text,
                Err(TextOperationError::IncorrectInputLength {
                    expected: 5,
                    actual: 8,
                })
            ));
        }

        #[test]
        fn test_transform_retain_identity() {
            let original_text = "Hello";
            let ops1 = vec![CompoundOp::Retain { count: 5 }];
            let ops2 = vec![CompoundOp::Retain { count: 5 }];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(op1_prime.ops, vec![CompoundOp::Retain { count: 5 }]);
            assert_eq!(op2_prime.ops, vec![CompoundOp::Retain { count: 5 }]);
        }

        #[test]
        fn test_transform_insert() {
            // Conflict resolves `op1` before `op2` to "Hello World", not " World Hello"
            let original_text = "";
            let ops1 = vec![CompoundOp::Insert {
                text: "Hello".to_string(),
            }];
            let ops2 = vec![CompoundOp::Insert {
                text: " World".to_string(),
            }];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(
                op1_prime.ops,
                vec![
                    CompoundOp::Insert {
                        text: "Hello".to_string()
                    },
                    CompoundOp::Retain { count: 6 }
                ]
            );
            assert_eq!(
                op2_prime.ops,
                vec![
                    CompoundOp::Retain { count: 5 },
                    CompoundOp::Insert {
                        text: " World".to_string()
                    }
                ]
            );
        }

        #[test]
        fn test_transform_insert_multilingual() {
            // Same as `test_transform_insert` but with multilingual text,
            // particularly where one char is multiple bytes in UTF-8.
            let original_text = "";
            let ops1 = vec![CompoundOp::Insert {
                text: "你好".to_string(),
            }];
            let ops2 = vec![CompoundOp::Insert {
                text: " 世界".to_string(),
            }];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(
                op1_prime.ops,
                vec![
                    CompoundOp::Insert {
                        text: "你好".to_string()
                    },
                    CompoundOp::Retain { count: 3 }
                ]
            );
            assert_eq!(
                op2_prime.ops,
                vec![
                    CompoundOp::Retain { count: 2 },
                    CompoundOp::Insert {
                        text: " 世界".to_string()
                    }
                ]
            );
        }

        #[test]
        fn test_transform_delete_retain() {
            let original_text = "Good";
            let ops1 = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Delete { count: 2 },
            ];
            let ops2 = vec![CompoundOp::Retain { count: 4 }];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(
                op1_prime.ops,
                vec![
                    CompoundOp::Retain { count: 2 },
                    CompoundOp::Delete { count: 2 },
                ]
            );
            assert_eq!(op2_prime.ops, vec![CompoundOp::Retain { count: 2 }]);
        }

        #[test]
        fn test_transform_retain_delete() {
            let original_text = "Goodbye";
            // Not most condensed `CompoundOp`, split only for testing
            let ops1 = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Retain { count: 5 },
            ];
            let ops2 = vec![
                CompoundOp::Retain { count: 4 },
                CompoundOp::Delete { count: 3 },
            ];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(op1_prime.ops, vec![CompoundOp::Retain { count: 4 }]);
            assert_eq!(
                op2_prime.ops,
                vec![
                    CompoundOp::Retain { count: 4 },
                    CompoundOp::Delete { count: 3 },
                ]
            );
        }

        #[test]
        fn test_transform_double_delete() {
            // Represents users deleting the same text, so delete applied only once. Output is "Good", not "G".
            let original_text = "Goodbye";
            let ops1 = vec![
                CompoundOp::Retain { count: 4 },
                CompoundOp::Delete { count: 3 },
            ];
            let ops2 = vec![
                CompoundOp::Retain { count: 4 },
                CompoundOp::Delete { count: 3 },
            ];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);

            let (op1_prime, op2_prime) = ops1.transform(&ops2).unwrap();
            assert_eq!(
                op2_prime
                    .apply(&ops1.apply(original_text).unwrap())
                    .unwrap(),
                op1_prime
                    .apply(&ops2.apply(original_text).unwrap())
                    .unwrap()
            );

            assert_eq!(op1_prime.ops, vec![CompoundOp::Retain { count: 4 },]);
            assert_eq!(op2_prime.ops, vec![CompoundOp::Retain { count: 4 },]);
        }

        #[test]
        fn test_basic_composition_1() {
            let ops1 = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Insert {
                    text: "world".to_string(),
                },
            ];
            let ops2 = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Delete { count: 3 },
                CompoundOp::Retain { count: 5 },
                CompoundOp::Insert {
                    text: "very".to_string(),
                },
            ];
            let input_text = "hello";
            let target_text = "heworldvery";

            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);
            let output_text = ops2.apply(&ops1.apply(input_text).unwrap()).unwrap();
            assert_eq!(output_text, target_text);

            let composed_op = ops1.compose(&ops2).unwrap();
            let output_text = composed_op.apply(input_text).unwrap();
            assert_eq!(output_text, target_text);
        }

        #[test]
        fn test_basic_composition_2() {
            let ops1 = vec![
                CompoundOp::Delete { count: 3 },
                CompoundOp::Insert {
                    text: "world".to_string(),
                },
                CompoundOp::Retain { count: 5 },
            ];
            let ops2 = vec![
                CompoundOp::Insert {
                    text: "very".to_string(),
                },
                CompoundOp::Retain { count: 3 },
                CompoundOp::Delete { count: 5 },
                CompoundOp::Retain { count: 2 },
            ];
            let input_text = "hi hello";
            let target_text = "veryworlo";

            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);
            let output_text = ops2.apply(&ops1.apply(input_text).unwrap()).unwrap();
            assert_eq!(output_text, target_text);

            let composed_op = ops1.compose(&ops2).unwrap();
            let output_text = composed_op.apply(input_text).unwrap();
            assert_eq!(output_text, target_text);
        }

        #[test]
        fn test_multilingual_composition() {
            // Tests the `TextOperation` struct can handle multilingual text, particularly
            // where one char is multiple bytes in UTF-8.

            // Op1 applied first, then op2
            let ops1 = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Insert {
                    text: " 世界".to_string(),
                },
            ];
            let ops2 = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Delete { count: 2 },
                CompoundOp::Insert {
                    text: ", 世".to_string(),
                },
                CompoundOp::Retain { count: 1 },
                CompoundOp::Insert {
                    text: "!".to_string(),
                },
            ];
            let input_text = "你好";
            let target_text = "你好, 世界!";

            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);
            let output_text = ops2.apply(&ops1.apply(input_text).unwrap()).unwrap();
            assert_eq!(output_text, target_text);

            let composed_op = ops1.compose(&ops2).unwrap();
            let output_text = composed_op.apply(input_text).unwrap();
            assert_eq!(output_text, target_text);
        }

        #[test]
        fn test_composition_incompatible() {
            let ops1 = vec![CompoundOp::Retain { count: 5 }];
            let ops2 = vec![CompoundOp::Retain { count: 6 }];
            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);
            assert!(matches!(
                ops1.compose(&ops2),
                Err(TextOperationError::CompositionIncompatible { .. })
            ));
        }

        #[test]
        fn test_composition_delete_overrides_insert() {
            let ops1 = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Insert {
                    text: "world".to_string(),
                },
            ];
            let ops2 = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 5 },
            ];

            let input_text = "hello";
            let target_text = "hello";

            let ops1 = TextOperation::from_ops(ops1.into_iter(), None, false);
            let ops2 = TextOperation::from_ops(ops2.into_iter(), None, false);
            let output_text = ops2.apply(&ops1.apply(input_text).unwrap()).unwrap();
            assert_eq!(output_text, target_text);

            let composed_op = ops1.compose(&ops2).unwrap();
            let output_text = composed_op.apply(input_text).unwrap();
            assert_eq!(output_text, target_text);
        }
    }

    // Test `TextUpdate`s generation. Test `TextUpdate` can be converted into a
    // `TextOperation` and the reverse where possible (i.e. where only insert,
    // delete, retain and not replace).
    mod text_update {
        use super::*;

        #[test]
        fn test_text_update_all_retain() {
            let updates = vec![];
            let start_doc = "Hello";
            let text_op = text_updates_to_text_operation(&updates, start_doc.chars().count());
            let expected_ops = vec![CompoundOp::Retain {
                count: start_doc.chars().count(),
            }];
            assert_eq!(text_op.ops, expected_ops);
            // Test `TextOperation` -> `TextUpdate` reverse operation
            assert_eq!(text_operation_text_updates(&text_op), updates);
        }

        #[test]
        fn test_text_insert_delete() {
            let updates = vec![
                TextUpdate::new(
                    TextUpdateRange::new(0, 0),
                    TextUpdateRange::new(0, 1),
                    "H".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(0, 1),
                    TextUpdateRange::new(1, 1),
                    "".to_string(),
                ),
            ];
            let start_doc = "ello";
            let target_doc = "Hllo";
            let text_op = text_updates_to_text_operation(&updates, target_doc.chars().count());
            let expected_ops = vec![
                CompoundOp::Insert {
                    text: "H".to_string(),
                },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Retain { count: 3 },
            ];
            assert_eq!(text_op.ops, expected_ops);
            assert_eq!(text_op.apply(start_doc).unwrap(), target_doc);
            // Test `TextOperation` -> `TextUpdate` reverse operation
            assert_eq!(text_operation_text_updates(&text_op), updates);
        }

        #[test]
        fn test_text_insert_delete_multilingual() {
            // Tests the `TextOperation` struct can handle multilingual text, particularly
            // where one char is multiple bytes in UTF-8.
            let updates = vec![
                TextUpdate::new(
                    TextUpdateRange::new(0, 0),
                    TextUpdateRange::new(0, 1),
                    "你".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(0, 1),
                    TextUpdateRange::new(1, 1),
                    "".to_string(),
                ),
            ];
            // Deliberately uses the character U+ff1f（？）instead of U+003f (?) to test multibyte characters
            let start_doc = "X好吗？";
            let target_doc = "你好吗？";
            let text_op = text_updates_to_text_operation(&updates, target_doc.chars().count());
            let expected_ops = vec![
                CompoundOp::Insert {
                    text: "你".to_string(),
                },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Retain { count: 3 },
            ];
            assert_eq!(text_op.ops, expected_ops);
            assert_eq!(text_op.apply(start_doc).unwrap(), target_doc);
            // Test `TextOperation` -> `TextUpdate` reverse operation
            assert_eq!(text_operation_text_updates(&text_op), updates);
        }

        #[test]
        fn test_replace() {
            // Notably, replace should put the insert before the delete so the user's cursor
            // is placed at the end of the replaced section
            let updates = vec![TextUpdate::new(
                TextUpdateRange::new(0, 1),
                TextUpdateRange::new(0, 1),
                "H".to_string(),
            )];
            let start_doc = "hi";
            let target_doc = "Hi";
            let text_op = text_updates_to_text_operation(&updates, target_doc.chars().count());
            let expected_ops = vec![
                CompoundOp::Insert {
                    text: "H".to_string(),
                },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Retain { count: 1 },
            ];
            assert_eq!(text_op.ops, expected_ops);
            assert_eq!(text_op.apply(start_doc).unwrap(), target_doc);
            // Reverse operation not tested here because `TextOperation` cannot represent a replace with one operation
        }

        #[test]
        fn test_text_multi_insert_delete() {
            let start_text = "Helloss wor!";
            let target_text = "Hello World!";
            let updates = vec![
                TextUpdate::new(
                    TextUpdateRange::new(5, 7),
                    TextUpdateRange::new(5, 5),
                    "".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(8, 9),
                    TextUpdateRange::new(6, 7),
                    "W".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(11, 11),
                    TextUpdateRange::new(9, 11),
                    "ld".to_string(),
                ),
            ];
            let expected_ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 2 },
                CompoundOp::Retain { count: 1 },
                CompoundOp::Insert {
                    text: "W".to_string(),
                },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Retain { count: 2 },
                CompoundOp::Insert {
                    text: "ld".to_string(),
                },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = text_updates_to_text_operation(&updates, target_text.chars().count());
            assert_eq!(text_op.ops, expected_ops);
            assert_eq!(text_op.apply(start_text).unwrap(), target_text);
            // `TextOperation` -> `TextUpdate` reverse operation not tested because this test uses replace operations
        }

        #[test]
        fn test_text_op_all_retain() {
            let ops = vec![CompoundOp::Retain { count: 5 }];
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let updates = text_operation_text_updates(&text_op);
            assert!(updates.is_empty());
        }

        #[test]
        fn test_text_op_insert() {
            let ops = vec![CompoundOp::Insert {
                text: "world".to_string(),
            }];
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let updates = text_operation_text_updates(&text_op);
            let expected_updates = vec![TextUpdate::new(
                TextUpdateRange::new(0, 0),
                TextUpdateRange::new(0, 5),
                "world".to_string(),
            )];
            assert_eq!(updates, expected_updates);
        }

        #[test]
        fn test_text_op_delete() {
            let ops = vec![CompoundOp::Delete { count: 3 }];
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let updates = text_operation_text_updates(&text_op);
            let expected_updates = vec![TextUpdate::new(
                TextUpdateRange::new(0, 3),
                TextUpdateRange::new(0, 0),
                "".to_string(),
            )];
            assert_eq!(updates, expected_updates);
        }

        #[test]
        fn test_text_op_multi_insert_delete() {
            let ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 2 },
                CompoundOp::Retain { count: 1 },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Insert {
                    text: "W".to_string(),
                },
                CompoundOp::Retain { count: 2 },
                CompoundOp::Insert {
                    text: "ld".to_string(),
                },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = TextOperation::from_ops(ops.into_iter(), None, false);
            let updates = text_operation_text_updates(&text_op);
            let expected_updates = vec![
                TextUpdate::new(
                    TextUpdateRange::new(5, 7),
                    TextUpdateRange::new(5, 5),
                    "".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(8, 9),
                    TextUpdateRange::new(6, 6),
                    "".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(9, 9),
                    TextUpdateRange::new(6, 7),
                    "W".to_string(),
                ),
                TextUpdate::new(
                    TextUpdateRange::new(11, 11),
                    TextUpdateRange::new(9, 11),
                    "ld".to_string(),
                ),
            ];
            assert_eq!(updates, expected_updates);
        }

        #[test]
        fn test_doc_to_text_update() {
            let doc = "Hello World!";
            let updates = text_update_from_doc(doc);
            let expected_updates = vec![TextUpdate::new(
                TextUpdateRange::new(0, 0),
                TextUpdateRange::new(0, doc.chars().count()),
                doc.to_string(),
            )];
            assert_eq!(updates, expected_updates);
        }

        #[test]
        fn test_doc_to_text_update_multilingual() {
            // Tests the `TextOperation` struct can handle multilingual text, particularly
            // where one char is multiple bytes in UTF-8.
            let doc = "你好世界！";
            let updates = text_update_from_doc(doc);
            let expected_updates = vec![TextUpdate::new(
                TextUpdateRange::new(0, 0),
                TextUpdateRange::new(0, doc.chars().count()),
                doc.to_string(),
            )];
            assert_eq!(updates, expected_updates);
        }
    }
}
