// src/fuzzy.rs

use crate::node::Node;

pub struct FuzzySearcher<'k, 'v, V> {
  // 'v: The lifetime of the data residing in the Trie
  results: Vec<(Vec<u8>, &'v V)>,
  // 'k: The lifetime of the search pattern (key)
  target: &'k [u8],
  max_distance: usize,
}

impl<'k, 'v, V> FuzzySearcher<'k, 'v, V> {
  pub fn new(target: &'k [u8], max_dist: usize) -> Self {
    Self {
      results: Vec::new(),
      target,
      max_distance: max_dist,
    }
  }

  pub fn search(mut self, root: &'v Node<V>) -> Vec<(Vec<u8>, &'v V)> {
    // Initial row: 0, 1, 2, ... len
    let row: Vec<usize> = (0..=self.target.len()).collect();
    let mut prefix = Vec::with_capacity(32);

    self.recursive_search(root, &row, &mut prefix);
    self.results
  }

  fn recursive_search(&mut self, node: &'v Node<V>, prev_row: &[usize], prefix: &mut Vec<u8>) {
    match node {
      Node::Internal { children, value } => {
        // 1. Check value at this node
        if let Some(v) = value {
          if prev_row[self.target.len()] <= self.max_distance {
            self.results.push((prefix.clone(), v));
          }
        }

        // 2. Iterate children
        for (char_code, child_opt) in children.iter().enumerate() {
          if let Some(child) = child_opt {
            let ch = char_code as u8;

            let next_row = self.calculate_next_row(prev_row, ch);

            // Optimization: Prune branch if min edit distance > max allowed
            if next_row.iter().min().unwrap() <= &self.max_distance {
              prefix.push(ch);
              self.recursive_search(child, &next_row, prefix);
              prefix.pop();
            }
          }
        }
      }
      Node::Radix {
        prefix: node_prefix,
        child,
      } => {
        let len_diff = (self.target.len() as isize - node_prefix.len() as isize).abs() as usize;
        // Get the minimum possible distance at this node
        let min_possible_dist = prev_row.iter().min().unwrap() + len_diff;

        if min_possible_dist > self.max_distance {
          // It's impossible for this prefix or any of its children to be a match.
          // Prune this entire branch.
          return;
        }

        // Calculate rows for the whole prefix
        let mut current_row = prev_row.to_vec();
        let mut valid = true;

        for &b in node_prefix {
          current_row = self.calculate_next_row(&current_row, b);
          // Optimization: if min distance exceeds max, we can stop early
          if current_row.iter().min().unwrap() > &self.max_distance {
            valid = false;
            break;
          }
        }

        if valid {
          prefix.extend_from_slice(node_prefix);
          self.recursive_search(child, &current_row, prefix);

          // Backtrack prefix
          let new_len = prefix.len() - node_prefix.len();
          prefix.truncate(new_len);
        }
      }
      Node::Leaf(leaf) => {
        for (suffix, val) in leaf.iter() {
          let mut current_row = prev_row.to_vec();
          let mut valid = true;
          for &b in suffix {
            current_row = self.calculate_next_row(&current_row, b);
            if current_row.iter().min().unwrap() > &self.max_distance {
              valid = false;
              break;
            }
          }
          if valid && current_row[self.target.len()] <= self.max_distance {
            let mut full_key = prefix.clone();
            full_key.extend_from_slice(suffix);
            self.results.push((full_key, val));
          }
        }
      }
    }
  }

  #[inline]
  fn calculate_next_row(&self, prev_row: &[usize], ch: u8) -> Vec<usize> {
    let columns = self.target.len() + 1;
    let mut next_row = vec![0; columns];
    next_row[0] = prev_row[0] + 1;

    for i in 1..columns {
      let insert_cost = next_row[i - 1] + 1;
      let delete_cost = prev_row[i] + 1;
      let replace_cost = prev_row[i - 1] + if self.target[i - 1] == ch { 0 } else { 1 };

      next_row[i] = insert_cost.min(delete_cost).min(replace_cost);
    }
    next_row
  }
}
