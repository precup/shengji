use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

use itertools::Itertools;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{Card, Trump};

pub type MatchingCards = Vec<(OrderedCard, usize)>;
pub type MatchingCardsRef = [(OrderedCard, usize)];
pub type AdjacentTupleSizes = Vec<usize>;
pub type PlayRequirements = Vec<AdjacentTupleSizes>;

/// A wrapper around a card with a given trump, which provides ordering characteristics.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OrderedCard {
    pub card: Card,
    pub trump: Trump,
}

impl std::fmt::Debug for OrderedCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.card)
    }
}

impl OrderedCard {
    pub fn successor(self) -> Vec<OrderedCard> {
        self.trump
            .successor(self.card)
            .into_iter()
            .map(|card| Self {
                card,
                trump: self.trump,
            })
            .collect()
    }

    pub fn make_map(
        cards: impl Iterator<Item = Card>,
        trump: Trump,
    ) -> BTreeMap<OrderedCard, usize> {
        let mut counts = BTreeMap::new();
        for card in cards {
            let card = OrderedCard { card, trump };
            *counts.entry(card).or_insert(0) += 1;
        }
        counts
    }

    pub fn card(self) -> Card {
        self.card
    }

    pub fn cards<'a, 'b: 'a>(
        iter: impl Iterator<Item = (&'b OrderedCard, &'b usize)> + 'a,
    ) -> impl Iterator<Item = &'b OrderedCard> + 'a {
        iter.flat_map(|(card, count)| (0..*count).map(move |_| card))
    }

    pub fn cmp_effective(self, o: OrderedCard) -> Ordering {
        debug_assert_eq!(self.trump, o.trump);
        self.trump.compare_effective(self.card, o.card)
    }
}

impl Ord for OrderedCard {
    fn cmp(&self, o: &OrderedCard) -> Ordering {
        self.trump.compare(self.card, o.card)
    }
}

impl PartialOrd for OrderedCard {
    fn partial_cmp(&self, o: &OrderedCard) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

type Usizes = Vec<usize>;

lazy_static::lazy_static! {
    static ref GROUP_CACHE: Mutex<HashMap<usize, Vec<AdjacentTupleSizes>>> = Mutex::new(HashMap::new());
    static ref SEQUENTIAL_ASSIGNMENT_CACHE: Mutex<HashMap<usize, Vec<Vec<Usizes>>>> = Mutex::new(HashMap::new());
    static ref FULL_DECOMPOSITION_CACHE: Mutex<HashMap<usize, Vec<PlayRequirements>>> = Mutex::new(HashMap::new());
}

pub fn subsequent_decomposition_ordering(
    mut adj_reqs: PlayRequirements,
    include_new_adjacency: bool,
) -> Vec<PlayRequirements> {
    if !adj_reqs.iter().all(|adj_req| !adj_req.is_empty()) {
        return vec![];
    }

    for adj_req in &mut adj_reqs {
        adj_req.sort_by(|a, b| b.cmp(a));
    }

    let mut decompositions = Vec::with_capacity(adj_reqs.len());
    for adj_req in &adj_reqs {
        let len = adj_req.iter().sum::<usize>();
        let mut decomp = full_decomposition_ordering(len);
        decomp.reverse();
        while let Some(v) = decomp.pop() {
            if v.len() == 1 && v.get(0) == Some(adj_req) {
                break;
            }
        }

        decompositions.push(decomp);
    }
    let mut subsequent_decomps = vec![];
    let mut current_decomps: HashMap<usize, PlayRequirements> = HashMap::new();
    for (i, adj_req) in adj_reqs.iter().enumerate() {
        current_decomps.insert(i, vec![adj_req.clone()]);
    }
    let can_include_new_adjacency = adj_reqs
        .iter()
        .map(|a| include_new_adjacency || a.len() > 1)
        .collect::<Vec<_>>();

    // Keep the indices of decompositions as a range to assist in the later loop.
    let mut h = (0..adj_reqs.len()).collect::<Vec<usize>>();

    loop {
        // Decompose the value with the most remaining decompositions.
        h.sort_by(|idx_a, idx_b| {
            decompositions
                .get(*idx_b)
                .map(|d| d.len())
                .unwrap_or(0)
                .cmp(&decompositions.get(*idx_a).map(|d| d.len()).unwrap_or(0))
        });
        let to_decompose = h.first();

        if let Some((idx, v)) = to_decompose.and_then(|i| {
            decompositions
                .get_mut(*i)
                .and_then(|v: &mut Vec<PlayRequirements>| v.pop())
                .map(|v: PlayRequirements| (i, v))
        }) {
            current_decomps.insert(*idx, v);
        } else {
            break;
        }
        // If we decomposed something which didn't include an adjacency requirement into
        // something which does, ensure that that's allowed by the caller.
        let include = h.iter().all(|i| {
            current_decomps[i]
                .iter()
                .all(|a| a.len() == 1 || can_include_new_adjacency[*i])
        });
        if include {
            let mut full_decomp = h
                .iter()
                .flat_map(|i| current_decomps[i].iter().cloned())
                .collect::<PlayRequirements>();
            full_decomp.sort_by(|a, b| b.cmp(a));
            subsequent_decomps.push(full_decomp);
        }
    }
    subsequent_decomps
}

///
/// Computes the full decomposition ordering for the number of cards specified.
///
/// The result is a list of sequences of adjacent card-lengths. Note: single cards are never
/// required to be adjacent.
///
pub fn full_decomposition_ordering(num_cards: usize) -> Vec<PlayRequirements> {
    assert!(num_cards >= 1);

    {
        let m = FULL_DECOMPOSITION_CACHE.lock().unwrap();
        if let Some(v) = m.get(&num_cards) {
            return v.clone();
        }
    }

    let groupings = find_tuple_partitions(num_cards);

    let mut full_decomp = vec![];

    for group in groupings {
        // Find the non-single cards
        let one_idx = group.iter().position(|v| *v == 1).unwrap_or(group.len());
        let gt_1 = &group[..one_idx];
        let eq_1 = &group[one_idx..];

        if gt_1.is_empty() {
            full_decomp.push(eq_1.iter().map(|v| vec![*v]).collect());
        } else {
            for mut decomposition in group_into_sequential_tuples(gt_1) {
                decomposition.extend(eq_1.iter().map(|v| vec![*v]));
                decomposition.sort_by(|a, b| b.cmp(a));
                full_decomp.push(decomposition);
            }
        }
    }
    let full_decomp: Vec<_> = full_decomp.into_iter().unique().collect();

    let mut m = FULL_DECOMPOSITION_CACHE.lock().unwrap();
    m.insert(num_cards, full_decomp.clone());

    full_decomp
}

/// For a given number of cards `num`, compute all of the different ways we
/// could break those cards up into smaller tuples, in descending order of
/// complexity.
///
/// e.g. find_tuple_partitions(4) gives
/// [[4], [3, 1], [2, 2], [2, 1, 1], [1, 1, 1, 1]]
///
fn find_tuple_partitions(num: usize) -> Vec<AdjacentTupleSizes> {
    assert!(num >= 1);
    {
        let m = GROUP_CACHE.lock().unwrap();
        if let Some(v) = m.get(&num) {
            return v.clone();
        }
    }
    let mut groupings = Vec::new();
    if num == 1 {
        groupings.push(vec![1]);
    } else {
        let smaller_groupings = find_tuple_partitions(num - 1);
        // try incrementing each smaller grouping
        for mut g in smaller_groupings {
            let mut incremented = HashSet::new();

            for v in &g {
                if !incremented.contains(v) {
                    incremented.insert(*v);
                    let mut found = false;
                    let mut g_ = vec![];
                    for vv in &g {
                        if *vv == *v && !found {
                            found = true;
                            g_.push(*vv + 1);
                        } else {
                            g_.push(*vv);
                        }
                    }
                    groupings.push(g_);
                }
            }

            groupings.push({
                g.push(1);
                g
            });
        }
    }
    groupings.sort_by(|a, b| b.cmp(a));
    groupings.dedup();

    let mut m = GROUP_CACHE.lock().unwrap();
    m.insert(num, groupings.clone());

    groupings
}

/// For a given slice of tuple-sizes, allocate them into all possible sequential
/// orderings.
///
/// e.g. group_into_sequential_tuples(&[3, 2, 2]) returns [
///     [[3, 2, 2]],
///     [[2, 3, 2]],
///     [[2, 2, 3]],
///     [[3, 2], [2]],
///     [[2, 3], [2]],
///     [[3], [2], [2]]
/// ]
///
/// The innermost vector is ordered, but the others are not.
///
fn group_into_sequential_tuples(values: &[usize]) -> Vec<PlayRequirements> {
    let assignments = compute_adjacent_assignments(values.len());
    assignments
        .into_iter()
        .flat_map(|assignment| {
            let assignment: Vec<Vec<usize>> = assignment
                .into_iter()
                .map(|subassignment| subassignment.into_iter().map(|idx| values[idx]).collect())
                .collect();

            if assignment.iter().all(|p| p.iter().all(|pp| *pp == p[0])) {
                vec![assignment]
            } else {
                assignment
                    .into_iter()
                    .map(|p| {
                        p.iter()
                            .copied()
                            .permutations(p.len())
                            .unique()
                            .collect::<Vec<_>>()
                    })
                    .multi_cartesian_product()
                    .collect()
            }
        })
        .unique()
        .collect()
}

fn compute_adjacent_assignments(length: usize) -> Vec<Vec<Usizes>> {
    assert!(length >= 1);
    if length == 1 {
        return vec![vec![vec![0]]];
    }

    {
        let m = SEQUENTIAL_ASSIGNMENT_CACHE.lock().unwrap();
        if let Some(seq) = m.get(&length).as_ref() {
            return seq.to_vec();
        }
    }

    let elem = length - 1;
    let shorter = compute_adjacent_assignments(length - 1);
    let mut assignments: Vec<Vec<Usizes>> = vec![];

    for mut part in shorter {
        for i in 0..part.len() {
            let list = part.get_mut(i).unwrap();
            list.push(elem);
            assignments.push(part.to_vec());
            let list = part.get_mut(i).unwrap();
            list.pop();
        }
        part.push(vec![elem]);
        assignments.push(part.to_vec());
        part.pop();
    }

    assignments.sort_by(|a, b| {
        let a_max_len = a.iter().map(|v| v.len()).max();
        let b_max_len = b.iter().map(|v| v.len()).max();

        b_max_len.cmp(&a_max_len).then(a.len().cmp(&b.len()))
    });
    assignments.dedup();

    let mut m = SEQUENTIAL_ASSIGNMENT_CACHE.lock().unwrap();
    m.insert(length, assignments.clone());
    assignments
}

#[cfg(test)]
mod tests {

    use super::{
        compute_adjacent_assignments, find_tuple_partitions, full_decomposition_ordering,
        subsequent_decomposition_ordering, PlayRequirements,
    };

    #[test]
    fn test_subsequent_decomposition_ordering() {
        let f = |r: PlayRequirements| -> Vec<Vec<Vec<usize>>> {
            subsequent_decomposition_ordering(r, true)
                .into_iter()
                .map(|x| x.iter().map(|y| y.to_vec()).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        let g = |r: PlayRequirements| -> Vec<Vec<Vec<usize>>> {
            subsequent_decomposition_ordering(r, false)
                .into_iter()
                .map(|x| x.iter().map(|y| y.to_vec()).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };

        assert!(f(vec![vec![1]]).is_empty());
        assert!(g(vec![vec![1]]).is_empty());
        assert_eq!(f(vec![vec![2]]), vec![vec![vec![1], vec![1]]]);
        assert_eq!(g(vec![vec![2]]), vec![vec![vec![1], vec![1]]]);
        assert_eq!(
            f(vec![vec![3]]),
            vec![vec![vec![2], vec![1]], vec![vec![1], vec![1], vec![1]]]
        );
        assert_eq!(
            g(vec![vec![3]]),
            vec![vec![vec![2], vec![1]], vec![vec![1], vec![1], vec![1]]]
        );
        assert_eq!(
            f(vec![vec![4]]),
            vec![
                vec![vec![3], vec![1]],
                vec![vec![2, 2]],
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            g(vec![vec![4]]),
            vec![
                vec![vec![3], vec![1]],
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(vec![vec![2, 2]]),
            vec![
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            g(vec![vec![2, 2]]),
            vec![
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(vec![vec![2], vec![2]]),
            vec![
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            g(vec![vec![2], vec![2]]),
            vec![
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(vec![vec![4]]),
            vec![
                vec![vec![3], vec![1]],
                vec![vec![2, 2]],
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]],
            ]
        );
        assert_eq!(
            g(vec![vec![4]]),
            vec![
                vec![vec![3], vec![1]],
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]],
            ]
        );
        assert_eq!(
            f(vec![vec![4, 4]]),
            vec![
                vec![vec![4], vec![4]],
                vec![vec![4, 3], vec![1]],
                vec![vec![3, 4], vec![1]],
                vec![vec![4], vec![3], vec![1]],
                vec![vec![4, 2, 2]],
                vec![vec![2, 4, 2]],
                vec![vec![2, 2, 4]],
                vec![vec![4, 2], vec![2]],
                vec![vec![2, 4], vec![2]],
                vec![vec![4], vec![2, 2]],
                vec![vec![4], vec![2], vec![2]],
                vec![vec![4, 2], vec![1], vec![1]],
                vec![vec![2, 4], vec![1], vec![1]],
                vec![vec![4], vec![2], vec![1], vec![1]],
                vec![vec![4], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![3, 3, 2]],
                vec![vec![3, 2, 3]],
                vec![vec![2, 3, 3]],
                vec![vec![3, 3], vec![2]],
                vec![vec![3, 2], vec![3]],
                vec![vec![3], vec![2, 3]],
                vec![vec![3], vec![3], vec![2]],
                vec![vec![3, 3], vec![1], vec![1]],
                vec![vec![3], vec![3], vec![1], vec![1]],
                vec![vec![3, 2, 2], vec![1]],
                vec![vec![2, 3, 2], vec![1]],
                vec![vec![2, 2, 3], vec![1]],
                vec![vec![3, 2], vec![2], vec![1]],
                vec![vec![2, 3], vec![2], vec![1]],
                vec![vec![3], vec![2, 2], vec![1]],
                vec![vec![3], vec![2], vec![2], vec![1]],
                vec![vec![3, 2], vec![1], vec![1], vec![1]],
                vec![vec![2, 3], vec![1], vec![1], vec![1]],
                vec![vec![3], vec![2], vec![1], vec![1], vec![1]],
                vec![vec![3], vec![1], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![2, 2, 2, 2]],
                vec![vec![2, 2, 2], vec![2]],
                vec![vec![2, 2], vec![2, 2]],
                vec![vec![2, 2], vec![2], vec![2]],
                vec![vec![2], vec![2], vec![2], vec![2]],
                vec![vec![2, 2, 2], vec![1], vec![1]],
                vec![vec![2, 2], vec![2], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![2], vec![1], vec![1]],
                vec![vec![2, 2], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![1], vec![1], vec![1], vec![1]],
                vec![
                    vec![2],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ],
                vec![
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ]
            ]
        );
        assert_eq!(
            g(vec![vec![4, 4]]),
            vec![
                vec![vec![4], vec![4]],
                vec![vec![4, 3], vec![1]],
                vec![vec![3, 4], vec![1]],
                vec![vec![4], vec![3], vec![1]],
                vec![vec![4, 2, 2]],
                vec![vec![2, 4, 2]],
                vec![vec![2, 2, 4]],
                vec![vec![4, 2], vec![2]],
                vec![vec![2, 4], vec![2]],
                vec![vec![4], vec![2, 2]],
                vec![vec![4], vec![2], vec![2]],
                vec![vec![4, 2], vec![1], vec![1]],
                vec![vec![2, 4], vec![1], vec![1]],
                vec![vec![4], vec![2], vec![1], vec![1]],
                vec![vec![4], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![3, 3, 2]],
                vec![vec![3, 2, 3]],
                vec![vec![2, 3, 3]],
                vec![vec![3, 3], vec![2]],
                vec![vec![3, 2], vec![3]],
                vec![vec![3], vec![2, 3]],
                vec![vec![3], vec![3], vec![2]],
                vec![vec![3, 3], vec![1], vec![1]],
                vec![vec![3], vec![3], vec![1], vec![1]],
                vec![vec![3, 2, 2], vec![1]],
                vec![vec![2, 3, 2], vec![1]],
                vec![vec![2, 2, 3], vec![1]],
                vec![vec![3, 2], vec![2], vec![1]],
                vec![vec![2, 3], vec![2], vec![1]],
                vec![vec![3], vec![2, 2], vec![1]],
                vec![vec![3], vec![2], vec![2], vec![1]],
                vec![vec![3, 2], vec![1], vec![1], vec![1]],
                vec![vec![2, 3], vec![1], vec![1], vec![1]],
                vec![vec![3], vec![2], vec![1], vec![1], vec![1]],
                vec![vec![3], vec![1], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![2, 2, 2, 2]],
                vec![vec![2, 2, 2], vec![2]],
                vec![vec![2, 2], vec![2, 2]],
                vec![vec![2, 2], vec![2], vec![2]],
                vec![vec![2], vec![2], vec![2], vec![2]],
                vec![vec![2, 2, 2], vec![1], vec![1]],
                vec![vec![2, 2], vec![2], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![2], vec![1], vec![1]],
                vec![vec![2, 2], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![1], vec![1], vec![1], vec![1]],
                vec![
                    vec![2],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ],
                vec![
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ]
            ]
        );

        assert_eq!(
            f(vec![vec![2, 2], vec![3], vec![2]]),
            vec![
                vec![vec![3], vec![2], vec![2], vec![2]],
                vec![vec![3], vec![2], vec![2], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![2], vec![1], vec![1], vec![1]],
                vec![
                    vec![2],
                    vec![2],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ],
                vec![
                    vec![2],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ],
                vec![
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1],
                    vec![1]
                ]
            ]
        );

        for i in 1..25 {
            // Construct all-ones
            let mut x = vec![];
            for _ in 0..i {
                x.push(vec![1]);
            }
            assert!(f(x.clone()).is_empty());

            // Construct all-3s
            let mut x = vec![];
            for _ in 0..i {
                x.push(vec![3]);
            }
            // Start with all 3s, a 2, and a 1
            let mut expected = vec![];
            for _ in 0..i - 1 {
                expected.push(vec![3]);
            }
            expected.push(vec![2]);
            expected.push(vec![1]);
            let mut res = f(x);
            res.reverse();
            while let Some(r) = res.pop() {
                assert_eq!(r, expected);
                // Replace a 3 with a 2 and a 1, or a 2 with a 1 and a 1.
                let mut v = expected.remove(0);
                assert!(v[0] >= 1);
                v[0] -= 1;
                let idx = match expected.iter().rposition(|z| *z == v) {
                    Some(idx) => idx,
                    None => {
                        assert!(res.is_empty());
                        break;
                    }
                };
                expected.insert(idx, v);
                expected.push(vec![1]);
            }
        }
    }

    #[test]
    fn test_full_decomposition_ordering() {
        let f = |n| -> Vec<Vec<Vec<usize>>> {
            full_decomposition_ordering(n)
                .into_iter()
                .map(|x| x.iter().map(|y| y.to_vec()).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(f(1), vec![vec![vec![1]]]);
        assert_eq!(f(2), vec![vec![vec![2]], vec![vec![1], vec![1]]]);
        assert_eq!(
            f(3),
            vec![
                vec![vec![3]],
                vec![vec![2], vec![1]],
                vec![vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(4),
            vec![
                vec![vec![4]],
                vec![vec![3], vec![1]],
                vec![vec![2, 2]],
                vec![vec![2], vec![2]],
                vec![vec![2], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(5),
            vec![
                vec![vec![5]],
                vec![vec![4], vec![1]],
                vec![vec![3, 2]],
                vec![vec![2, 3]],
                vec![vec![3], vec![2]],
                vec![vec![3], vec![1], vec![1]],
                vec![vec![2, 2], vec![1]],
                vec![vec![2], vec![2], vec![1]],
                vec![vec![2], vec![1], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1], vec![1]]
            ]
        );
        assert_eq!(
            f(6),
            vec![
                vec![vec![6]],
                vec![vec![5], vec![1]],
                vec![vec![4, 2]],
                vec![vec![2, 4]],
                vec![vec![4], vec![2]],
                vec![vec![4], vec![1], vec![1]],
                vec![vec![3, 3]],
                vec![vec![3], vec![3]],
                vec![vec![3, 2], vec![1]],
                vec![vec![2, 3], vec![1]],
                vec![vec![3], vec![2], vec![1]],
                vec![vec![3], vec![1], vec![1], vec![1]],
                vec![vec![2, 2, 2]],
                vec![vec![2, 2], vec![2]],
                vec![vec![2], vec![2], vec![2]],
                vec![vec![2, 2], vec![1], vec![1]],
                vec![vec![2], vec![2], vec![1], vec![1]],
                vec![vec![2], vec![1], vec![1], vec![1], vec![1]],
                vec![vec![1], vec![1], vec![1], vec![1], vec![1], vec![1]]
            ]
        );
    }

    #[test]
    fn test_compute_adjacent_assignments() {
        let f = |n| -> Vec<Vec<Vec<usize>>> {
            compute_adjacent_assignments(n)
                .into_iter()
                .map(|x| x.iter().map(|y| y.to_vec()).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(f(1), vec![vec![vec![0]]]);
        assert_eq!(f(2), vec![vec![vec![0, 1]], vec![vec![0], vec![1]]]);
        assert_eq!(
            f(3),
            vec![
                vec![vec![0, 1, 2]],
                vec![vec![0, 1], vec![2]],
                vec![vec![0, 2], vec![1]],
                vec![vec![0], vec![1, 2]],
                vec![vec![0], vec![1], vec![2]]
            ]
        );
        assert_eq!(
            f(4),
            vec![
                vec![vec![0, 1, 2, 3]],
                vec![vec![0, 1, 2], vec![3]],
                vec![vec![0, 1, 3], vec![2]],
                vec![vec![0, 2, 3], vec![1]],
                vec![vec![0], vec![1, 2, 3]],
                vec![vec![0, 1], vec![2, 3]],
                vec![vec![0, 2], vec![1, 3]],
                vec![vec![0, 3], vec![1, 2]],
                vec![vec![0, 1], vec![2], vec![3]],
                vec![vec![0, 2], vec![1], vec![3]],
                vec![vec![0], vec![1, 2], vec![3]],
                vec![vec![0, 3], vec![1], vec![2]],
                vec![vec![0], vec![1, 3], vec![2]],
                vec![vec![0], vec![1], vec![2, 3]],
                vec![vec![0], vec![1], vec![2], vec![3]]
            ]
        );
    }

    #[test]
    fn test_find_tuple_partitions() {
        let f = |n| -> Vec<Vec<usize>> {
            find_tuple_partitions(n)
                .into_iter()
                .map(|x| x.to_vec())
                .collect::<Vec<_>>()
        };
        assert_eq!(f(1), vec![vec![1]]);
        assert_eq!(f(2), vec![vec![2], vec![1, 1]]);
        assert_eq!(f(3), vec![vec![3], vec![2, 1], vec![1, 1, 1]]);

        assert_eq!(
            f(4),
            vec![
                vec![4],
                vec![3, 1],
                vec![2, 2],
                vec![2, 1, 1],
                vec![1, 1, 1, 1]
            ]
        );

        assert_eq!(
            f(5),
            vec![
                vec![5],
                vec![4, 1],
                vec![3, 2],
                vec![3, 1, 1],
                vec![2, 2, 1],
                vec![2, 1, 1, 1],
                vec![1, 1, 1, 1, 1]
            ]
        );
        assert_eq!(
            f(6),
            vec![
                vec![6],
                vec![5, 1],
                vec![4, 2],
                vec![4, 1, 1],
                vec![3, 3],
                vec![3, 2, 1],
                vec![3, 1, 1, 1],
                vec![2, 2, 2],
                vec![2, 2, 1, 1],
                vec![2, 1, 1, 1, 1],
                vec![1, 1, 1, 1, 1, 1]
            ]
        );
    }
}
