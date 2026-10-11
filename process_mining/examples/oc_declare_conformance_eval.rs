use process_mining::core::process_models::oc_declare::{OCDeclareArcType, ObjectTypeAssociation};
use process_mining::{
    conformance::object_centric::oc_declare::{
        discover_time_interval, evaluate_arc_conformance, oc_declare_conformance, evaluate_arc_score_components,
    },
    core::event_data::object_centric::linked_ocel::SlimLinkedOCEL,
    core::process_models::oc_declare::OCDeclareArcLabel,
    Importable, OCEL,
};
use std::{
    env,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

use process_mining::discovery::object_centric::oc_declare::{
    discover_behavior_constraints, OCDeclareDiscoveryOptions, OCDeclareReductionMode,
};
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
        //("Logistics", path.join("ContainerLogistics.json")),
        //("P2P", path.join("ocel2-p2p.json")),
        //("P2P-Weighted", path.join("ocel2_weighted.json")),
        (
        "BPIC2017",
        PathBuf::from("/Users/zixuanlu/Desktop/dataset/bpic2017-o2o-workflow-qualifier-index-no-ev-attrs.xml"),
    ),
        (
        "OrderManagement",
        PathBuf::from("/Users/zixuanlu/Desktop/dataset/order-management.json"),
    ),
    ];

    let csv_file = File::create("conformance_scores.csv")?;
    let mut writer = BufWriter::new(csv_file);
    writeln!(
        writer,
        "dataset,noise,from_event,to_event,std_score,object_score,temporal_score,combined_score"
    )?;

    for (name, log_path) in event_logs {
        if !log_path.exists() {
            println!("File {:?} not found, skipping...", log_path);
            continue;
        }

        println!("--- Evaluating Conformance on {} ---", name);
        let ocel = OCEL::import_from_path(&log_path).expect("Failed to import OCEL.");

        // before move ocel into locel, print some basic statistics
        println!("Total events: {}", ocel.events.len());
        println!("Total objects: {}", ocel.objects.len());

        let linked_ocel = SlimLinkedOCEL::from(ocel);
        //go through noise threshholds
        let noise_thresholds = vec![0.0, 0.1];

        for noise in noise_thresholds {
            println!(
                "\n>>> Running Discovery with Noise Threshold: {} <<<",
                noise
            );

            //set max 20 rules
            let mut options = OCDeclareDiscoveryOptions::default();

            options.noise_threshold = noise;
            options.counts_for_generation = (Some(1), Some(20));
            options.counts_for_filter = (Some(1), Some(20));

            options.considered_arrow_types = [
                OCDeclareArcType::EF,
                OCDeclareArcType::EP,
                OCDeclareArcType::AS,
            ]
            .into_iter()
            .collect();

            options.reduction = OCDeclareReductionMode::Lossless;
            options.refinement = true;
            //dynamic mine the rules
            let discovered_rules = discover_behavior_constraints(&linked_ocel, options);
            println!("Discovered {} rules.", discovered_rules.len());
            for (i, rule) in discovered_rules.iter().enumerate() {
                let from_act = rule.from.as_str();
                let to_act = rule.to.as_str();

                if (i + 1) % 5 == 0 || i + 1 == discovered_rules.len() {
                    println!("Processed {}/{} rules", i + 1, discovered_rules.len());
                }

                let std_score = oc_declare_conformance(&linked_ocel, rule);

                let scores = evaluate_arc_score_components(
                    from_act,
                    to_act,
                    &rule.label,
                    rule.arc_type,
                    &linked_ocel,
                );

                let fine_score = scores.combined_score;

                if i < 5 {
                    println!(
                        "Rule: {} -> {} | Std: {:.4} | Obj: {:.4} | Time: {:.4} | Combined: {:.4}",
                        from_act,
                        to_act,
                        std_score,
                        scores.object_score,
                        scores.temporal_score,
                        scores.combined_score,
                    );
                }

                // write results to CSV
                writeln!(
                    writer,
                    "{},{},{},{},{:.4},{:.4},{:.4},{:.4}",
                    name,
                    noise,
                    from_act,
                    to_act,
                    std_score,
                    scores.object_score,
                    scores.temporal_score,
                    fine_score,
                )?;
            }
        }
    }

    writer.flush()?;
    println!("\nDone! Results saved to conformance_scores_comparison.csv");
    Ok(())
}
/*   let count_from = ocel.events.iter().filter(|e| e.event_type == from_act).count();
    let count_to = ocel.events.iter().filter(|e| e.event_type == to_act).count();
    println!("Activity count -> {}: {}, {}: {}", from_act, count_from, to_act, count_to);

    let obj_count = ocel.objects.iter().filter(|o| o.object_type == obj_type).count();
    println!("Object count of type '{}': {}", obj_type, obj_count);
    // ------------------------------------------------

    // 1. use ocel.event_types from ocel to get the event types
    let event_types: Vec<String> = ocel.event_types.iter().map(|et| et.name.clone()).collect();

    // 2. change ocel to locel（这一步会消耗掉 ocel，所以必须放在打印后面）
    let locel = SlimLinkedOCEL::from_ocel(ocel);
        if event_types.len() < 2 {
            println!("Dataset {} has less than 2 event types, skipping.", name);
            continue;
        }

        //let from_act = &event_types[0];
        //let to_act = &event_types[1];
        let item_assoc = ObjectTypeAssociation::new_simple(obj_type);
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
}*/
