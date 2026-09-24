// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

use keyhammer_legacy::FuzzyIndex;

fn main() {
    let terms = vec![
        "javascript", "typescript", "python", "rust", "golang",
        "java", "kotlin", "swift", "ruby", "haskell",
    ];

    let index = FuzzyIndex::build(&terms, 2).unwrap();

    println!("Index stats: {:?}\n", index.stats());

    let queries = ["javasript", "typscript", "pythn", "ruts", "golanf"];

    for q in queries {
        let results = index.search(q, 3).unwrap();
        println!("\"{}\":", q);
        for r in &results {
            println!("  {} (score: {:.2}, hamming: {})", r.term, r.score, r.hamming_distance);
        }
        println!();
    }
}
