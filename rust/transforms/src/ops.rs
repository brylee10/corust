use std::{cmp::Ordering, collections::VecDeque};

use serde::{Deserialize, Serialize};

use crate::xforms::TextOperation;

// Code to test the operational transformation algorithm
#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub enum Op {
    // Insert at current index, advances index
    Insert {
        text: char,
    },
    // Delete at current index, does not change index
    Delete,
    #[default]
    // Advance current index
    Retain,
    // Both source and target are empty strings. No operations needed.
    EmptyStrings,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub enum CompoundOp {
    Insert { text: String },
    Delete { count: usize },
    Retain { count: usize },
}

impl CompoundOp {
    fn ops_to_compound_ops<T: Iterator<Item = Op>, B: FromIterator<CompoundOp>>(ops: T) -> B {
        let mut compound_ops = Vec::new();
        let mut last_op = None;
        for op in ops {
            match op {
                Op::Insert { text } => match last_op {
                    Some(CompoundOp::Insert { text: last_text }) => {
                        last_op = Some(CompoundOp::Insert {
                            text: format!("{}{}", last_text, text),
                        });
                    }
                    _ => {
                        if let Some(last_op) = last_op {
                            compound_ops.push(last_op);
                        }
                        last_op = Some(CompoundOp::Insert {
                            text: text.to_string(),
                        });
                    }
                },
                Op::Delete => match last_op {
                    Some(CompoundOp::Delete { count }) => {
                        last_op = Some(CompoundOp::Delete { count: count + 1 });
                    }
                    _ => {
                        if let Some(last_op) = last_op {
                            compound_ops.push(last_op);
                        }
                        last_op = Some(CompoundOp::Delete { count: 1 });
                    }
                },
                Op::Retain => match last_op {
                    Some(CompoundOp::Retain { count }) => {
                        last_op = Some(CompoundOp::Retain { count: count + 1 });
                    }
                    _ => {
                        if let Some(last_op) = last_op {
                            compound_ops.push(last_op);
                        }
                        last_op = Some(CompoundOp::Retain { count: 1 });
                    }
                },
                Op::EmptyStrings => {}
            }
        }
        if let Some(last_op) = last_op {
            compound_ops.push(last_op);
        }
        B::from_iter(compound_ops)
    }

    // Returns the number of characters the operation produces.
    // Useful for calculating the output document size of a text operation.
    pub(crate) fn op_output_len(&self) -> isize {
        match self {
            CompoundOp::Insert { text } => isize::try_from(text.chars().count()).unwrap(),
            CompoundOp::Delete { .. } => 0,
            CompoundOp::Retain { count } => isize::try_from(*count).unwrap(),
        }
    }

    // Calculates the expected input text segment this operation operates on. Useful for sanity checks.
    pub(crate) fn expected_input_size(&self) -> usize {
        match self {
            CompoundOp::Insert { .. } => 0,
            CompoundOp::Delete { count } => *count,
            CompoundOp::Retain { count } => *count,
        }
    }
}

#[derive(Default, Clone, Debug)]
struct EditDistOp {
    source: (usize, usize),
    cost: usize,
    op: Op,
}

impl Eq for EditDistOp {}

impl PartialEq for EditDistOp {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl PartialOrd for EditDistOp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EditDistOp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost.cmp(&other.cost)
    }
}

/// Returns operations for minimal edit distance from old_text to new_text as `TextOperation`
/// Text operation is newly derived, so there is no previous id or transform
pub fn text_ops(old_text: &str, new_text: &str) -> TextOperation {
    let ops = edit_ops(old_text, new_text);
    TextOperation::from_ops(ops.into_iter(), None, false)
}

/// Returns operations for minimal edit distance from old_text to new_text
/// using only deletes, inserts, retains (no modifies)
/// Uses a heuristic that small, contiguous portions of large documents are edited at once
/// from proposal here: https://fitzgeraldnick.com/2011/04/05/operational-transformation-operations.html
/// The output edit operation is still optimal.
pub fn edit_ops(old_text: &str, new_text: &str) -> VecDeque<CompoundOp> {
    let old_chars = old_text.chars().collect::<Vec<_>>();
    let new_chars = new_text.chars().collect::<Vec<_>>();

    // Number of retain operations at the start of the edit
    // Conceptually, [0, start_retains - 1) are retain operations (ignoring when `start_retains = 0`)
    let start_retains = old_chars
        .iter()
        .zip(new_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Number of retain operations at the end of the edit
    // Conceptually, [end_idx + 1, n) are retain operations (ignoring when `end_idx = n - 1`)
    let end_retains = old_chars[start_retains..]
        .iter()
        .rev()
        .zip(new_chars[start_retains..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let m = old_chars.len() - end_retains - start_retains;
    let n = new_chars.len() - end_retains - start_retains;

    // Edit distance is computing edit operations by filling a `m x n` matrix (operations to go from `old_chars[0..i]` to `new_chars[0..j]`)
    let mut dp: Vec<Vec<EditDistOp>> = vec![vec![EditDistOp::default(); n + 1]; m + 1];

    // Fill the matrix (i, j are 1 indexed)
    for i in 0..=m {
        for j in 0..=n {
            if i == 0 && j == 0 {
                dp[i][j] = EditDistOp {
                    source: (0, 0),
                    cost: 0,
                    op: Op::EmptyStrings,
                };
            } else if i == 0 {
                dp[i][j] = EditDistOp {
                    source: (0, j - 1),
                    cost: j,
                    // Translate to zero indexed position
                    op: Op::Insert {
                        text: new_chars[j - 1 + start_retains],
                    },
                };
            } else if j == 0 {
                dp[i][j] = EditDistOp {
                    source: (i - 1, 0),
                    cost: i,
                    op: Op::Delete,
                };
            } else if old_chars[i - 1 + start_retains] == new_chars[j - 1 + start_retains] {
                dp[i][j] = EditDistOp {
                    source: (i - 1, j - 1),
                    cost: dp[i - 1][j - 1].cost,
                    op: Op::Retain,
                };
            } else if dp[i - 1][j].cost < dp[i][j - 1].cost {
                dp[i][j] = EditDistOp {
                    source: (i - 1, j),
                    cost: dp[i - 1][j].cost + 1,
                    op: Op::Delete,
                };
            } else {
                dp[i][j] = EditDistOp {
                    source: (i, j - 1),
                    cost: dp[i][j - 1].cost + 1,
                    op: Op::Insert {
                        text: new_chars[j - 1 + start_retains],
                    },
                };
            }
        }
    }

    // Traverse matrix to get operations for minimal transform
    let mut ops = Vec::new();

    let mut curr_pos = (m, n);
    while curr_pos != (0, 0) {
        let next_op = dp[curr_pos.0][curr_pos.1].source;
        ops.push(dp[curr_pos.0][curr_pos.1].op.clone());
        curr_pos = next_op;
    }
    ops.reverse();

    let mut compound_ops: VecDeque<CompoundOp> = CompoundOp::ops_to_compound_ops(ops.into_iter());
    if start_retains > 0 {
        let start_retains = CompoundOp::Retain {
            count: start_retains,
        };
        compound_ops.push_front(start_retains);
    }
    if end_retains > 0 {
        let end_retains = CompoundOp::Retain { count: end_retains };
        compound_ops.push_back(end_retains);
    }
    compound_ops
}
#[cfg(test)]
mod test {
    use super::*;

    mod test_edit_distance {
        use super::*;

        #[test]
        fn test_empty_edit() {
            let old_text = "";
            let new_text = "";
            let expected_ops = vec![];
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_identical() {
            let old_text = "hello";
            let new_text = "hello";
            let expected_ops = vec![Op::Retain, Op::Retain, Op::Retain, Op::Retain, Op::Retain];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(expected_ops.into_iter());

            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_empty_old() {
            let old_text = "";
            let new_text = "hello";
            let ops = vec![
                Op::Insert { text: 'h' },
                Op::Insert { text: 'e' },
                Op::Insert { text: 'l' },
                Op::Insert { text: 'l' },
                Op::Insert { text: 'o' },
            ];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_empty_new() {
            let old_text = "hello";
            let new_text = "";
            let ops = vec![Op::Delete, Op::Delete, Op::Delete, Op::Delete, Op::Delete];

            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_edit_ops_1() {
            let old_text = "hello";
            let new_text = "mellow";
            let ops = vec![
                Op::Delete,
                Op::Insert { text: 'm' },
                Op::Retain,
                Op::Retain,
                Op::Retain,
                Op::Retain,
                Op::Insert { text: 'w' },
            ];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_edit_ops_2() {
            let old_text = "no common";
            let new_text = "zebra";
            let ops = vec![
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Insert { text: 'z' },
                Op::Insert { text: 'e' },
                Op::Insert { text: 'b' },
                Op::Insert { text: 'r' },
                Op::Insert { text: 'a' },
            ];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_edit_ops_3() {
            let old_text = "Hi, Nice to meet";
            let new_text = "Hi! Nice to meet you.";
            let expected_ops = vec![
                CompoundOp::Retain { count: 2 },
                CompoundOp::Delete { count: 1 },
                CompoundOp::Insert {
                    text: '!'.to_string(),
                },
                CompoundOp::Retain { count: 13 },
                CompoundOp::Insert {
                    text: " you.".to_string(),
                },
            ];
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_edit_ops_intuitive_1() {
            // Edit should take as many retains as possible first
            // Should not give `insert("o, n") but `insert(", no")`
            let old_text = "go";
            let new_text = "go, no";
            let ops = vec![
                Op::Retain,
                Op::Retain,
                Op::Insert { text: ',' },
                Op::Insert { text: ' ' },
                Op::Insert { text: 'n' },
                Op::Insert { text: 'o' },
            ];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }

        #[test]
        fn test_edit_ops_intuitive_2() {
            // Edit should put deletes before inserts when both are equally minimal
            let old_text = "goodbye";
            let new_text = "goodnight";
            let ops = vec![
                Op::Retain,
                Op::Retain,
                Op::Retain,
                Op::Retain,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Insert { text: 'n' },
                Op::Insert { text: 'i' },
                Op::Insert { text: 'g' },
                Op::Insert { text: 'h' },
                Op::Insert { text: 't' },
            ];
            let expected_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(edit_ops(old_text, new_text), expected_ops);
        }
    }

    mod compound_ops {
        use super::*;

        #[test]
        fn test_convert_to_compount_ops() {
            let ops = vec![
                Op::Insert { text: 'h' },
                Op::Insert { text: 'e' },
                Op::Insert { text: 'l' },
                Op::Insert { text: 'l' },
                Op::Insert { text: 'o' },
                Op::Delete,
                Op::Delete,
                Op::Retain,
                Op::Retain,
                Op::Retain,
                Op::Delete,
                Op::Delete,
                Op::Delete,
                Op::Insert { text: 'h' },
                Op::Insert { text: 'i' },
                Op::Retain,
                Op::Retain,
            ];
            let expected_compound_ops = vec![
                CompoundOp::Insert {
                    text: "hello".to_string(),
                },
                CompoundOp::Delete { count: 2 },
                CompoundOp::Retain { count: 3 },
                CompoundOp::Delete { count: 3 },
                CompoundOp::Insert {
                    text: "hi".to_string(),
                },
                CompoundOp::Retain { count: 2 },
            ];
            let compound_ops: Vec<_> = CompoundOp::ops_to_compound_ops(ops.into_iter());
            assert_eq!(compound_ops, expected_compound_ops);
        }
    }
}
