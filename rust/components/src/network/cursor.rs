//! Cursor position in the document
//! `from, to`: the range of the selection, where `from` is the start and `to` is the end
//! `anchor, head`: the position of the cursor, where `anchor` is the cursor location and
//! `head` is the free moving end of the selection
//!
//! Invariants: `from <= to`, `anchor = from or anchor = to`, `head = from or head = to`
//!
//! # Transformations
//! Includes transformations which apply [`TextOperation`]s to [`CursorPos`] positions.
//! Generalization of the [`TextOperation`] which natively applies to strings.
//!
//! # Cursor indexing:
//! Indexing is 0 indexed with the cursor to the left of the character with the same index.
//!
//! For example, the string "hello" has the following indices, where the numbers correspond
//! to cursor indices.
//! ```text
//! 0"h"1"e"2"e"3"l"4"l"5"o"6
//! ```
//!
use anyhow::Result;
use fnv::FnvHashMap;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use corust_transforms::{ops::CompoundOp, xforms::TextOperation};
use thiserror::Error;

use super::UserId;

pub type CursorMap = FnvHashMap<UserId, CursorPos>;

/// Cursor position indices are in characters and not bytes
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CursorPos {
    from: usize,
    to: usize,
    anchor: usize,
    head: usize,
}

#[wasm_bindgen]
impl CursorPos {
    pub fn new(from: usize, to: usize, anchor: usize, head: usize) -> Self {
        CursorPos {
            from,
            to,
            anchor,
            head,
        }
    }

    pub fn from(&self) -> usize {
        self.from
    }

    pub fn to(&self) -> usize {
        self.to
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn head(&self) -> usize {
        self.head
    }

    /// Used by javascript for printing
    // Ignore lint because this is a wasm_bindgen function
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        format!("{:?}", self)
    }

    pub fn equals(&self, other: &CursorPos) -> bool {
        self == other
    }

    pub fn highlight_len(&self) -> usize {
        self.to - self.from
    }
}

#[derive(Error, Debug)]
pub enum CursorTransformError {
    #[error(
        "Text op input length {text_op_input_len} but cursor beyond input length {cursor_pos:?}"
    )]
    OutOfBounds {
        text_op_input_len: usize,
        cursor_pos: CursorPos,
    },
    #[error(
        "Value mismatch for user id {user_id}: server val {server_val:?} != client val {client_val:?}"
    )]
    ValueMismatch {
        user_id: UserId,
        server_val: CursorPos,
        client_val: CursorPos,
    },
}

/// Utility which applies a `TextOperation` to each `CursorPos` in a `CursorMap`.
/// Returns early if a cursor transformation error occurs for any `CursorPos`.
pub fn transform_cursor_map(
    text_operation: &TextOperation,
    cursor_map: &CursorMap,
) -> Result<CursorMap, CursorTransformError> {
    let mut new_cursor_map = FnvHashMap::default();
    for (user_id, cursor_pos) in cursor_map.iter() {
        let new_cursor_pos = transform_cursor(text_operation, cursor_pos)?;
        new_cursor_map.insert(*user_id, new_cursor_pos);
    }
    Ok(new_cursor_map)
}

/// Transform a cursor position given a `TextOperation`.
///
/// There is the concept of bias for the `from` (start) and `to` (end) of the cursor range. Definitions:
/// - `from` index bias: an insertion at index `i` with cursor (from) at index `i`` could cause the cursor to move
///   to `i + 1` (forward bias) or stay at `i` (backwards bias).
/// - `to` index bias: when `from != to`, an insertion at index `i` with `to` at index `i` could cause `to` to
///   increment to `i + 1` (forward bias) or stay at `i` (backwards bias)
///
/// This cursor transformation algorithm assumes a forward bias for `from` and a backwards bias for `to`. The effect is
/// typing at either the start or end of a highlighted range does not expand the range. This reflects what the CodeMirror
/// editor natively does for highlighted ranges during collaboration.
///
/// Reference on "forward bias"
/// https://marijnhaverbeke.nl/blog/collaborative-editing-cm.html
pub fn transform_cursor(
    text_operation: &TextOperation,
    cursor_pos: &CursorPos,
) -> Result<CursorPos, CursorTransformError> {
    if text_operation.input_length() < cursor_pos.to() {
        log::error!(
            "Text operation: {:?}, Cursor Pos: {:?}",
            text_operation,
            cursor_pos
        );
        return Err(CursorTransformError::OutOfBounds {
            text_op_input_len: text_operation.input_length(),
            cursor_pos: *cursor_pos,
        });
    }

    debug_assert!(
        cursor_pos.from() <= cursor_pos.to(),
        "Cursor range is invalid, got from: {}, to: {}",
        cursor_pos.from(),
        cursor_pos.to()
    );

    debug_assert!(
        cursor_pos.anchor() == cursor_pos.from() || cursor_pos.anchor() == cursor_pos.to(),
        "Cursor anchor should be either `from` or `to` but got anchor: {}, from: {}, to: {}",
        cursor_pos.anchor(),
        cursor_pos.from(),
        cursor_pos.to()
    );

    debug_assert!(
        cursor_pos.head() == cursor_pos.from() || cursor_pos.head() == cursor_pos.to(),
        "Cursor head should be either `from` or `to` but got head: {}, from: {}, to: {}",
        cursor_pos.head(),
        cursor_pos.from(),
        cursor_pos.to()
    );

    // Is `from` the anchor or `to` the anchor?
    let from_anchor = cursor_pos.anchor() == cursor_pos.from();

    // Number of indices to shift `from` and `to` cursor indices
    let mut new_cursor_pos_from = cursor_pos.from();
    let mut new_cursor_pos_to = cursor_pos.to();
    // Size of "gap" between start and end of a highlight
    let mut highlight_len = cursor_pos.to() - cursor_pos.from();
    // Index in previous string (only delete and retains advance), not to be confused with index into new string
    // (only insert and retains advance)
    let mut idx_old_str = 0;
    for compound_op in text_operation.ops() {
        match compound_op {
            CompoundOp::Retain { count } => {
                idx_old_str += count;
            }
            CompoundOp::Insert { text } => {
                if idx_old_str <= cursor_pos.from() {
                    // Shifts entire range
                    // `<=` because transform assumes forward bias for the `from` index
                    // Insertion at the range start shifts range to the right but does not expand the range
                    new_cursor_pos_from += text.chars().count();
                    new_cursor_pos_to += text.chars().count();
                } else if idx_old_str < cursor_pos.to() {
                    // Shifts only the end of the range
                    // `<` because transform assumes backwards bias for the end index
                    // Insertion at the end of the range does not shift the end of the range to the right so it
                    // does not expand the highlighted range
                    new_cursor_pos_to += text.chars().count();
                    highlight_len += text.chars().count();
                } else {
                    // Insertion after the range does not affect cursor index
                }
                // Insertions do not change index into old string
            }
            CompoundOp::Delete { count } => {
                let delete_range_end = idx_old_str + count;
                if idx_old_str <= cursor_pos.from() {
                    // Deletions at or before the start of the range shift
                    // the `from` at most to `idx` and shift `to` at most to `idx`
                    new_cursor_pos_from -= std::cmp::min(*count, cursor_pos.from() - idx_old_str);
                    new_cursor_pos_to -= std::cmp::min(*count, cursor_pos.to() - idx_old_str);
                    // Highlight len shrinks on `=` because deletion at index `from` is the character right after the cursor
                    if cursor_pos.from() <= delete_range_end {
                        highlight_len -=
                            std::cmp::min(highlight_len, delete_range_end - cursor_pos.from());
                    }
                } else if idx_old_str <= cursor_pos.to() {
                    // Deletions within the range shift the end of the range to the left
                    let shift_to = std::cmp::min(*count, cursor_pos.to() - idx_old_str);
                    new_cursor_pos_to -= shift_to;
                    highlight_len -= shift_to;
                } else {
                    // Deletions after the range do not affect cursor index
                }
                idx_old_str += count;
            }
        }
    }
    let (new_cursor_pos_anchor, new_cursor_pos_head) = if from_anchor {
        (new_cursor_pos_from, new_cursor_pos_to)
    } else {
        (new_cursor_pos_to, new_cursor_pos_from)
    };
    debug_assert!(
        new_cursor_pos_to - new_cursor_pos_from == highlight_len,
        "Highlight length mismatch. Expected: {}, Actual: {}",
        highlight_len,
        new_cursor_pos_to - new_cursor_pos_from
    );

    Ok(CursorPos::new(
        new_cursor_pos_from,
        new_cursor_pos_to,
        new_cursor_pos_anchor,
        new_cursor_pos_head,
    ))
}

#[cfg(debug_assertions)]
#[inline]
pub fn sanity_check_overlapping_keys_match(
    server_map: &CursorMap,
    client_map: &CursorMap,
) -> Result<()> {
    // CAN REMOVE: Unsure if this sanity check is true, we'll see
    // Cursor positions of overlapping keys should be the same between server and client
    // Only checks overlapping because the same users may not be present in both maps
    // (e.g. the client types in the local document so its cursor is in its local map, but
    // the server did not receive this client update yet so the client is not in the server map)
    for (user_id, server_val) in server_map.iter() {
        if let Some(client_val) = client_map.get(user_id) {
            if client_val != server_val {
                Err(CursorTransformError::ValueMismatch {
                    user_id: *user_id,
                    server_val: *server_val,
                    client_val: *client_val,
                })?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod no_highlight {
        use super::*;
        // Tests that assume the user is not highlighting text

        #[test]
        fn test_transform_cursor_noop_retain() {
            // Test an all retain operation does not change the cursor position
            // Doc: "Hel|lo!"
            let text = "Hello!";
            let expected_transformed_text = "Hello!";
            let starting_cursor_pos = CursorPos::new(3, 3, 3, 3);
            let expected_transformed_cursor_pos = CursorPos::new(3, 3, 3, 3);

            let compound_op = CompoundOp::Retain { count: 6 };
            let text_op = TextOperation::from_ops(std::iter::once(compound_op), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_before_cursor() {
            // An insert before the cursor should shift the cursor to the right
            // In Doc: "Wor|ld!"
            let text = "World!";
            // Out Doc: "Hello Wor|ld!"
            let expected_transformed_text = "Hello World!";

            let starting_cursor_pos = CursorPos::new(3, 3, 3, 3);
            let expected_transformed_cursor_pos = CursorPos::new(9, 9, 9, 9);

            let compound_ops = vec![
                CompoundOp::Insert {
                    text: "Hello ".to_string(),
                },
                CompoundOp::Retain { count: 6 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_before_cursor() {
            // A delete before the cursor should shift the cursor to the left
            // In Doc: "Hello Wor|ld!"
            let text = "Hello World!";
            // Out Doc: "Wor|ld!"
            let expected_transformed_text = "World!";

            let starting_cursor_pos = CursorPos::new(9, 9, 9, 9);
            let expected_transformed_cursor_pos = CursorPos::new(3, 3, 3, 3);

            let compound_ops = vec![
                CompoundOp::Delete { count: 6 },
                CompoundOp::Retain { count: 6 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_over_cursor() {
            // A delete with a range including the position of the cursor should shift the cursor to the end of the range
            // (which is equivalent to the end of the start of the range, since the delete collapses the start and end to the same point).
            // In Doc: "Hello Wor|ld!"
            let text = "Hello World!";
            // Out Doc: "Hello|!"
            let expected_transformed_text = "Hello!";

            let starting_cursor_pos = CursorPos::new(9, 9, 9, 9);
            let expected_transformed_cursor_pos = CursorPos::new(5, 5, 5, 5);

            let compound_ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 6 },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_forward_bias() {
            // An insert before the cursor should shift the cursor to the right
            // In Doc: "Wor|d!"
            let text = "Word!";
            // Out Doc: "Worl|d!"
            let expected_transformed_text = "World!";

            let starting_cursor_pos = CursorPos::new(3, 3, 3, 3);
            let expected_transformed_cursor_pos = CursorPos::new(4, 4, 4, 4);

            let compound_ops = vec![
                CompoundOp::Retain { count: 3 },
                CompoundOp::Insert {
                    text: "l".to_string(),
                },
                CompoundOp::Retain { count: 2 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_at_cursor() {
            // A delete at the cursor should shift the cursor to the left
            // In Doc: "ab|c"
            let text = "abc";
            // Out Doc: "a|c"
            let expected_transformed_text = "ac";

            let starting_cursor_pos = CursorPos::new(2, 2, 2, 2);
            let expected_transformed_cursor_pos = CursorPos::new(1, 1, 1, 1);

            let compound_ops = vec![
                CompoundOp::Retain { count: 1 },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Retain { count: 1 },
            ];

            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_delete_at_cursor_multilingual() {
            // Tests that the cursor transformation works with multilingual text
            // in particular where a character is multiple bytes long
            // A delete at the cursor should shift the cursor to the left
            // In Doc: "你好吗。|"
            let text = "你好吗。";
            // Out Doc: "你好吗？"
            let expected_transformed_text = "你好吗？";

            let starting_cursor_pos = CursorPos::new(4, 4, 4, 4);
            let expected_transformed_cursor_pos = CursorPos::new(4, 4, 4, 4);

            let compound_ops = vec![
                CompoundOp::Retain { count: 3 },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Insert {
                    // Deliberately uses the U+ff1f "？" instead of U+003f "?" to test multi-byte characters
                    text: "？".to_string(),
                },
            ];

            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }
    }

    mod highlight {
        use super::*;

        // Tests that assume the user is highlighting text
        #[test]
        fn test_text_op_error() {
            // Test that an error is returned if the cursor is out of bounds of the text operation
            // Doc: "Hello! |"
            let text = "Hello!";
            let expected_transformed_text = "Hello!";
            let starting_cursor_pos = CursorPos::new(7, 7, 7, 7);

            let compound_op = CompoundOp::Retain { count: 6 };
            let text_op = TextOperation::from_ops(std::iter::once(compound_op), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos);
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert!(matches!(
                new_cursor_pos,
                Err(CursorTransformError::OutOfBounds { .. })
            ));
        }

        #[test]
        fn test_insert_forward_bias() {
            // An insert with a range that is entirely before a cursor highlight should shift the cursor range
            // In Doc: "|World!|"
            let text = "World!";
            // Out Doc: "Hello |World!|"
            let expected_transformed_text = "Hello World!";

            let starting_cursor_pos = CursorPos::new(0, 6, 0, 6);
            let expected_transformed_cursor_pos = CursorPos::new(6, 12, 6, 12);

            let compound_ops = vec![
                CompoundOp::Insert {
                    text: "Hello ".to_string(),
                },
                CompoundOp::Retain { count: 6 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_inside_cursor_range() {
            // An insert with a range that is entirely within a cursor highlight should expand change the cursor range
            // In Doc: "|The jumped over the moon.|"
            let text = "The jumped over the moon.";
            // Out Doc: "|The cow jumped over the moon.|"
            let expected_transformed_text = "The cow jumped over the moon.";

            let starting_cursor_pos = CursorPos::new(0, 25, 0, 25);
            let expected_transformed_cursor_pos = CursorPos::new(0, 29, 0, 29);

            let compound_ops = vec![
                CompoundOp::Retain { count: 4 },
                CompoundOp::Insert {
                    text: "cow ".to_string(),
                },
                CompoundOp::Retain { count: 21 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_inside_cursor_range() {
            // A delete with a range that is entirely within a cursor highlight should shrink the cursor range
            // In Doc: "|Hello World!|"
            let text = "Hello World!";
            // Out Doc: "|Hello!|"
            let expected_transformed_text = "Hello!";

            let starting_cursor_pos = CursorPos::new(0, 12, 0, 12);
            let expected_transformed_cursor_pos = CursorPos::new(0, 6, 0, 6);

            let compound_ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 6 },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_partial_cursor_range_upper() {
            // A delete with a range that partially overlaps with upper part of a cursor highlight
            // should shrink the cursor range in the overlapping region
            // In Doc: "|Hello Wo|rld!"
            let text = "Hello World!";
            // Out Doc: "|Hello|!"
            let expected_transformed_text = "Hello!";

            let starting_cursor_pos = CursorPos::new(0, 8, 0, 8);
            let expected_transformed_cursor_pos = CursorPos::new(0, 5, 0, 5);

            let compound_ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Delete { count: 6 },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_partial_cursor_range_lower() {
            // A delete with a range that partially overlaps with lower part of a cursor highlight
            // should shrink the cursor range in the overlapping region
            // In Doc: "Hel|lo World!|"
            let text = "Hello World!";
            // Out Doc: "|World!|"
            let expected_transformed_text = "World!";

            let starting_cursor_pos = CursorPos::new(3, 12, 3, 12);
            let expected_transformed_cursor_pos = CursorPos::new(0, 6, 0, 6);

            let compound_ops = vec![
                CompoundOp::Delete { count: 6 },
                CompoundOp::Retain { count: 6 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_delete_entire_cursor_range() {
            // A delete with a range that entirely overlaps with a cursor highlight
            // should shrink the cursor range to gap length
            // In Doc: "|Goodbye!|"
            let text = "Goodbye!";
            // Out Doc: "|"
            let expected_transformed_text = "";

            let starting_cursor_pos = CursorPos::new(0, 8, 0, 8);
            let expected_transformed_cursor_pos = CursorPos::new(0, 0, 0, 0);

            let compound_ops = vec![CompoundOp::Delete { count: 8 }];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_range_end() {
            // An insert at the end of the highlighted range should not change the cursor range
            // Tests backwards bias for the `to` index

            // In Doc: "|Hello|"
            let text = "Hello";
            // Out Doc: "|Hello| World!"
            let expected_transformed_text = "Hello World!";

            let starting_cursor_pos = CursorPos::new(0, 5, 0, 5);
            let expected_transformed_cursor_pos = CursorPos::new(0, 5, 0, 5);

            let compound_ops = vec![
                CompoundOp::Retain { count: 5 },
                CompoundOp::Insert {
                    text: " World!".to_string(),
                },
            ];

            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }

        #[test]
        fn test_insert_multilingual() {
            // Tests insert in a highlighted range with multilingual text where characters
            // are more than one byte long
            // In Doc: "|你！|"
            let text = "你！";
            // Out Doc: "|你好！|"
            let expected_transformed_text = "你好！";

            let starting_cursor_pos = CursorPos::new(0, 2, 0, 2);
            let expected_transformed_cursor_pos = CursorPos::new(0, 3, 0, 3);

            let compound_ops = vec![
                CompoundOp::Retain { count: 1 },
                CompoundOp::Insert {
                    text: "好".to_string(),
                },
                CompoundOp::Retain { count: 1 },
            ];
            let text_op = TextOperation::from_ops(compound_ops.into_iter(), None, false);
            let new_cursor_pos = transform_cursor(&text_op, &starting_cursor_pos).unwrap();
            let transformed_text = text_op.apply(text).unwrap();
            assert_eq!(transformed_text, expected_transformed_text);
            assert_eq!(new_cursor_pos, expected_transformed_cursor_pos);
        }
    }
}
