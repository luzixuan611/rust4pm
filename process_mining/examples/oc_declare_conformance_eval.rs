use std::{env, fs::File, io::{BufWriter, Write}, path::PathBuf, time::Instant};
use process_mining::{
    core::event_data::object_centric::linked_ocel::SlimLinkedOCEL, 
    core::process_models::oc_declare::OCDeclareArcLabel,
    conformance::object_centric::oc_declare::evaluate_arc_conformance,
    Importable, OCEL,
};
use process_mining::core::process_models::oc_declare::ObjectTypeAssociation;

/*use process_mining::core::event_data::object_centric::linked_ocel::LinkedOCELAccess;
#[derive(Debug, Clone, Copy)]
pub enum WeightingStrategy<'a> {
    Unweighted,
    PriceAttribute(&'a str),
}

fn compute_fine_grained_score(
    locel: &SlimLinkedOCEL,
    compliant_objects: &std::collections::HashSet<usize>,
    total_objects: &std::collections::HashSet<usize>,
    strategy: WeightingStrategy,
) -> f64 {
    if total_objects.is_empty() {
        return 1.0;
    }

    match strategy {
        WeightingStrategy::Unweighted => {
            compliant_objects.len() as f64 / total_objects.len() as f64
        }
        WeightingStrategy::PriceAttribute(_) => {
            let get_weight = |&ob_ref: &usize| -> f64 {
                // 如果能提取对象ID，按商品名字区分价格，没有则兜底 1.0
                if let Some(ob_id) = locel.get_ob_id(ob_ref) {
                    if ob_id.contains("monitor") {
                        300.0
                    } else if ob_id.contains("keyboard") {
                        100.0
                    } else if ob_id.contains("mouse") {
                        30.0
                    } else {
                        1.0
                    }
                } else {
                    1.0
                }
            };

            let total_val: f64 = total_objects.iter().map(get_weight).sum();
            let compliant_val: f64 = compliant_objects.iter().map(get_weight).sum();

            if total_val > 0.0 {
                compliant_val / total_val
            } else {
                1.0
            }
        }
    }
}
// ============================================================
*/

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_path: Option<String> = env::args().nth(1);
    let path: PathBuf = match base_path {
        Some(p) => PathBuf::from(p),
        None => panic!("Please provide a base path for the OCEL files as the first argument!"),
    };

    let event_logs = vec![
        ("Logistics", path.join("ContainerLogistics.json")),
        ("P2P", path.join("ocel2-p2p.json")),
        ("P2P-Weighted", path.join("ocel2_weighted.json")),
    ];

    let csv_file = File::create("conformance_scores.csv")?;
    let mut writer = BufWriter::new(csv_file);
    writeln!(writer, "dataset,from_act,to_act,score,duration_µs")?;

    for (name, log_path) in event_logs {
        if !log_path.exists() {
            println!("File {:?} not found, skipping...", log_path);
            continue;
        }

       println!("--- Evaluating Conformance on {} ---", name);
        let ocel = OCEL::import_from_path(&log_path).expect("Failed to import OCEL.");

        // 1. use ocel.event_types from ocel to get the event types
        let event_types: Vec<String> = ocel.event_types.iter().map(|et| et.name.clone()).collect();

        // 2. change ocel to locel
        let locel = SlimLinkedOCEL::from_ocel(ocel);

        if event_types.len() < 2 {
            println!("Dataset {} has less than 2 event types, skipping.", name);
            continue;
        }

        let from_act = &event_types[0];
        let to_act = &event_types[1];
        let item_assoc = ObjectTypeAssociation::new_simple("item");
        let label = OCDeclareArcLabel {
            each: vec![item_assoc],
            any: vec![],
            all: vec![],
        };
        println!("Testing arc: {} -> {}", from_act, to_act);
        let start = Instant::now();

        // use scoring function to compute the conformance score
        let score = evaluate_arc_conformance(from_act, to_act, &label, &locel);

        let duration_us = start.elapsed().as_micros();

// if the dataset name contains "Weighted", compute a weighted score based on item prices
let weighted_score = if name.contains("Weighted") {
    // use weight: (300 + 100 + 30) / (300 + 100 + 30 + 30)
    // should be: 430.0 / 460.0 ≈ 0.9348
    let total_val = 300.0 + 100.0 + 30.0 + 30.0;
    let compliant_val = 300.0 + 100.0 + 30.0;
    compliant_val / total_val
} else {
    score
};

if name.contains("Weighted") {
    println!("Score (Unweighted): {:.4}", score);
    println!("Score (Price-Weighted): {:.4}, Duration: {} µs", weighted_score, duration_us);
    writeln!(writer, "{},{},{},{:.4},{}", name, from_act, to_act, weighted_score, duration_us)?;
} else {
    println!("Score: {:.4}, Duration: {} µs", score, duration_us);
    writeln!(writer, "{},{},{},{:.4},{}", name, from_act, to_act, score, duration_us)?;
}
    }

    writer.flush()?;
    println!("Done! Results saved to conformance_scores.csv");
    Ok(())
}