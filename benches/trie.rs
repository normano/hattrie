use bench_matrix::{
  AbstractCombination, MatrixCellValue, criterion_runner::sync_suite::SyncBenchmarkSuite,
};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use hattrie::HatTrie;
use rand::{Rng, SeedableRng, prelude::SliceRandom, seq::IndexedRandom};
use rand_pcg::Pcg64;
use std::{
  collections::{BTreeMap, HashMap},
  hint::black_box,
  time::{Duration, Instant},
};

// ============================================================================
// 1. Configuration & State Definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Algo {
  HatTrie,
  HatTrieStream,
  HatTrieSegment,
  BTree,
  HashMap,
}

#[derive(Debug, Clone, PartialEq)]
enum Operation {
  Insert,
  Get,
  Iter,
  Range,
  Fuzzy(usize),
}

#[derive(Debug, Clone)]
struct BenchConfig {
  algo: Algo,
  op: Operation,
  count: usize,
  key_type: String,
  burst_threshold: usize,
}

enum Store {
  Hat(HatTrie<u64>),
  BTree(BTreeMap<Vec<u8>, u64>),
  Hash(HashMap<Vec<u8>, u64>),
  Empty,
}

struct BenchState {
  store: Store,
  keys: Vec<Vec<u8>>,
  search_keys: Vec<Vec<u8>>,
}

type BenchContext = ();

// ============================================================================
// 2. Helpers & Generators
// ============================================================================

fn generate_data(count: usize, key_type: &str) -> Vec<Vec<u8>> {
  let mut rng = Pcg64::seed_from_u64(42);

  if key_type == "Url" {
    let domains = ["com", "org", "net", "io", "gov"];
    let paths = ["blog", "api", "app", "login", "user", "dashboard"];
    (0..count)
      .map(|_| {
        let dom = domains.choose(&mut rng).unwrap();
        let p1 = paths.choose(&mut rng).unwrap();
        let p2 = paths.choose(&mut rng).unwrap();
        let id: u32 = rng.random();
        format!("https://www.example.{}/{}/{}/{}", dom, p1, p2, id).into_bytes()
      })
      .collect()
  } else {
    (0..count)
      .map(|_| {
        (0..16)
          .map(|_| rng.sample(rand::distr::Alphanumeric) as u8)
          .collect()
      })
      .collect()
  }
}

// ============================================================================
// 3. Matrix Extractor
// ============================================================================

fn extract_config(combo: &AbstractCombination) -> Result<BenchConfig, String> {
  let algo_str = combo.get_string(0)?;
  let algo = match algo_str {
    "HatTrie" => Algo::HatTrie,
    "HatTrieStream" => Algo::HatTrieStream,
    "HatTrieSegment" => Algo::HatTrieSegment,
    "BTree" => Algo::BTree,
    "HashMap" => Algo::HashMap,
    _ => return Err(format!("Unknown Algo: {}", algo_str)),
  };

  let op_str = combo.get_string(1)?;
  let op = if op_str == "Insert" {
    Operation::Insert
  } else if op_str == "Get" {
    Operation::Get
  } else if op_str == "Iter" {
    Operation::Iter
  } else if op_str == "Range" {
    Operation::Range
  } else if op_str.starts_with("Fuzzy") {
    let dist = op_str.split('-').nth(1).unwrap_or("1").parse().unwrap_or(1);
    Operation::Fuzzy(dist)
  } else {
    return Err(format!("Unknown Op: {}", op_str));
  };

  let count = combo.get_u64(2)? as usize;
  let key_type = combo.get_string(3)?.to_string();
  let burst_threshold = combo.get_u64(4).unwrap_or(0) as usize;

  Ok(BenchConfig {
    algo,
    op,
    count,
    key_type,
    burst_threshold,
  })
}

// ============================================================================
// 4. Setup & Logic
// ============================================================================

fn setup_fn(cfg: &BenchConfig) -> Result<(BenchContext, BenchState), String> {
  let keys = generate_data(cfg.count, &cfg.key_type);

  let mut search_keys = keys.clone();
  let mut rng = Pcg64::seed_from_u64(999);
  search_keys.shuffle(&mut rng);

  let store = if cfg.op == Operation::Insert {
    Store::Empty
  } else {
    match cfg.algo {
      Algo::HatTrie | Algo::HatTrieStream | Algo::HatTrieSegment => {
        let mut t = HatTrie::with_threshold(cfg.burst_threshold);
        for (i, k) in keys.iter().enumerate() {
          t.insert(k, i as u64);
        }
        Store::Hat(t)
      }
      Algo::BTree => {
        let mut t = BTreeMap::new();
        for (i, k) in keys.iter().enumerate() {
          t.insert(k.clone(), i as u64);
        }
        Store::BTree(t)
      }
      Algo::HashMap => {
        let mut t = HashMap::new();
        for (i, k) in keys.iter().enumerate() {
          t.insert(k.clone(), i as u64);
        }
        Store::Hash(t)
      }
    }
  };

  Ok((
    (),
    BenchState {
      store,
      keys,
      search_keys,
    },
  ))
}

fn benchmark_logic(
  _ctx: BenchContext,
  state: BenchState,
  cfg: &BenchConfig,
) -> (BenchContext, BenchState, Duration) {
  let start = Instant::now();

  match cfg.op {
    Operation::Insert => match cfg.algo {
      Algo::HatTrie => {
        let mut t = HatTrie::with_threshold(cfg.burst_threshold);
        for (i, k) in state.keys.iter().enumerate() {
          t.insert(k, i as u64);
        }
        black_box(t);
      }
      Algo::BTree => {
        let mut t = BTreeMap::new();
        for (i, k) in state.keys.iter().enumerate() {
          t.insert(k.clone(), i as u64);
        }
        black_box(t);
      }
      Algo::HashMap => {
        let mut t = HashMap::new();
        for (i, k) in state.keys.iter().enumerate() {
          t.insert(k.clone(), i as u64);
        }
        black_box(t);
      }
      _ => unreachable!(),
    },
    Operation::Get => match &state.store {
      Store::Hat(t) => {
        for k in &state.search_keys {
          black_box(t.get(k));
        }
      }
      Store::BTree(t) => {
        for k in &state.search_keys {
          black_box(t.get(k));
        }
      }
      Store::Hash(t) => {
        for k in &state.search_keys {
          black_box(t.get(k));
        }
      }
      _ => unreachable!(),
    },
    Operation::Iter => match &state.store {
      Store::Hat(t) => match cfg.algo {
        Algo::HatTrieStream => {
          let mut iter = t.streaming_iter();
          while let Some(item) = iter.next() {
            black_box((item.key(), item.value()));
          }
        }
        Algo::HatTrieSegment => {
          let mut iter = t.iter_segments();
          while let Some(item) = iter.next() {
            black_box((item.segments(), item.value()));
          }
        }
        Algo::HatTrie => {
          black_box(t.iter().count());
        }
        _ => unreachable!(),
      },
      Store::BTree(t) => {
        black_box(t.iter().count());
      }
      _ => unreachable!(),
    },
    Operation::Range => {
      let start_bound = &state.keys[state.keys.len() / 2];
      let start_slice: &[u8] = start_bound.as_slice();
      let bounds = (
        std::ops::Bound::Included(start_slice),
        std::ops::Bound::Unbounded,
      );

      match &state.store {
        Store::Hat(t) => {
          let count = t.range::<_, [u8]>(bounds).take(100).count();
          black_box(count);
        }
        Store::BTree(t) => {
          let count = t.range::<[u8], _>(bounds).take(100).count();
          black_box(count);
        }
        _ => unreachable!(),
      }
    }
    Operation::Fuzzy(dist) => {
      if let Store::Hat(t) = &state.store {
        let target = &state.search_keys[0];
        let res = t.fuzzy_search(target, dist);
        black_box(res);
      }
    }
  }

  let elapsed = start.elapsed();
  ((), state, elapsed)
}

fn teardown_fn(_: BenchContext, _: BenchState, _: &BenchConfig) {}

// ============================================================================
// 5. Suite Definitions
// ============================================================================

fn run_benchmarks(c: &mut Criterion) {
  let param_names = vec![
    "Algo".to_string(),
    "Op".to_string(),
    "Count".to_string(),
    "Type".to_string(),
    "Threshold".to_string(),
  ];

  let threshold_axis = vec![
    MatrixCellValue::Unsigned(512),
    MatrixCellValue::Unsigned(1024),
    MatrixCellValue::Unsigned(4096),
    MatrixCellValue::Unsigned(16384),
  ];

  // --- Suite 1: HatTrie Core ---
  let hattrie_core_axes = vec![
    vec![MatrixCellValue::String("HatTrie".to_string())],
    vec![
      MatrixCellValue::String("Insert".to_string()),
      MatrixCellValue::String("Get".to_string()),
    ],
    vec![MatrixCellValue::Unsigned(100_000)],
    vec![
      MatrixCellValue::String("Random".to_string()),
      MatrixCellValue::String("Url".to_string()),
    ],
    threshold_axis.clone(),
  ];

  SyncBenchmarkSuite::new(
    c,
    "Core-HatTrie".to_string(),
    Some(param_names.clone()),
    hattrie_core_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .throughput(|cfg| Throughput::Elements(cfg.count as u64))
  .run();

  // --- Suite 2: Reference Core (BTree and HashMap) ---
  let reference_core_axes = vec![
    vec![
      MatrixCellValue::String("BTree".to_string()),
      MatrixCellValue::String("HashMap".to_string()),
    ],
    vec![
      MatrixCellValue::String("Insert".to_string()),
      MatrixCellValue::String("Get".to_string()),
    ],
    vec![MatrixCellValue::Unsigned(100_000)],
    vec![
      MatrixCellValue::String("Random".to_string()),
      MatrixCellValue::String("Url".to_string()),
    ],
    vec![MatrixCellValue::Unsigned(0)], // Dummy axis for alignment
  ];

  SyncBenchmarkSuite::new(
    c,
    "Core-Reference".to_string(),
    Some(param_names.clone()),
    reference_core_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .throughput(|cfg| Throughput::Elements(cfg.count as u64))
  .run();

  // --- Suite 3: HatTrie Ordered Ops ---
  let hattrie_ordered_axes = vec![
    vec![MatrixCellValue::String("HatTrie".to_string())],
    vec![MatrixCellValue::String("Range".to_string())],
    vec![MatrixCellValue::Unsigned(100_000)],
    vec![MatrixCellValue::String("Random".to_string())],
    threshold_axis.clone(),
  ];

  SyncBenchmarkSuite::new(
    c,
    "Ordered-HatTrie".to_string(),
    Some(param_names.clone()),
    hattrie_ordered_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .throughput(|_| Throughput::Elements(100))
  .run();

  // --- Suite 4: Reference Ordered Ops (BTree only) ---
  let btree_ordered_axes = vec![
    vec![MatrixCellValue::String("BTree".to_string())],
    vec![
      // REMOVED: MatrixCellValue::String("Iter".to_string()),
      MatrixCellValue::String("Range".to_string()),
    ],
    vec![MatrixCellValue::Unsigned(100_000)],
    vec![MatrixCellValue::String("Random".to_string())],
    vec![MatrixCellValue::Unsigned(0)],
  ];

  SyncBenchmarkSuite::new(
    c,
    "Ordered-Reference".to_string(),
    Some(param_names.clone()),
    btree_ordered_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .throughput(|cfg| {
    // Throughput logic is simpler now
    Throughput::Elements(100)
  })
  .run();

  // --- Suite 5: Iteration Comparison ---
  let iter_axes = vec![
    vec![
      MatrixCellValue::String("HatTrie".to_string()),
      MatrixCellValue::String("HatTrieStream".to_string()),
      MatrixCellValue::String("HatTrieSegment".to_string()),
      MatrixCellValue::String("BTree".to_string()),
    ],
    vec![MatrixCellValue::String("Iter".to_string())],
    vec![MatrixCellValue::Unsigned(100_000)],
    vec![MatrixCellValue::String("Random".to_string())],
    threshold_axis.clone(), // Note: BTree will ignore this, which is fine
  ];

  SyncBenchmarkSuite::new(
    c,
    "Iteration".to_string(),
    Some(param_names.clone()),
    iter_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .throughput(|cfg| Throughput::Elements(cfg.count as u64))
  .run();

  // --- Suite 6: Fuzzy Search ---
  let fuzzy_axes = vec![
    vec![MatrixCellValue::String("HatTrie".to_string())],
    vec![
      MatrixCellValue::String("Fuzzy-1".to_string()),
      MatrixCellValue::String("Fuzzy-2".to_string()),
    ],
    vec![MatrixCellValue::Unsigned(10_000)],
    vec![MatrixCellValue::String("Random".to_string())],
    threshold_axis.clone(),
  ];

  SyncBenchmarkSuite::new(
    c,
    "Specialized-HatTrie".to_string(),
    Some(param_names),
    fuzzy_axes,
    Box::new(extract_config),
    setup_fn,
    benchmark_logic,
    teardown_fn,
  )
  .run();
}

criterion_group!(benches, run_benchmarks);
criterion_main!(benches);
