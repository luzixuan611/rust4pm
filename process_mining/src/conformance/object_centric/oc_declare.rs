//! Checking OC-DECLARE constraints against object-centric event data
use std::sync::atomic::AtomicU32;

use rustc_hash::FxHashSet;

use crate::core::{
    event_data::object_centric::linked_ocel::{
        e2o_rev_type_index::{E2ORevByTypeIndex, E2ORevTypeView},
        slim_linked_ocel::ObjectIndex,
        LinkedOCELAccess, SlimLinkedOCEL,
    },
    process_models::oc_declare::{
        EventOrSynthetic, OCDeclareArc, OCDeclareArcLabel, OCDeclareArcType, SetFilter,
    },
};

use chrono::{DateTime, Duration, FixedOffset};
use macros_process_mining::register_binding;
use rayon::prelude::*;

use crate::core::process_models::oc_declare::TimeInterval; // Import TimeInterval from the appropriate module

/// Get all events of the given event type satisfying the filters
///
/// `view` is `etype` resolved against an [`E2ORevByTypeIndex`]; `None` falls back to scanning.
pub(crate) fn target_events_for_binding<'a>(
    objs: &'a [SetFilter<&ObjectIndex>],
    linked_ocel: &'a SlimLinkedOCEL,
    etype: &'a str,
    view: Option<E2ORevTypeView<'a>>,
) -> impl Iterator<Item = EventOrSynthetic> + use<'a> {
    let for_ob = move |ob: ObjectIndex| -> Box<dyn Iterator<Item = EventOrSynthetic> + 'a> {
        match view {
            Some(view) => Box::new(view.events_of(ob).map(EventOrSynthetic::Event)),
            None => EventOrSynthetic::get_all_of_et_for_ob(linked_ocel, etype, ob),
        }
    };
    let initial: Box<dyn Iterator<Item = EventOrSynthetic>> = if objs.is_empty() {
        Box::new(EventOrSynthetic::get_all_syn_evs(linked_ocel, etype).into_iter())
    } else {
        match &objs[0] {
            // ANY is a union: an event referencing several of the objects still counts once.
            SetFilter::Any(items) if items.len() > 1 => {
                let mut seen: FxHashSet<EventOrSynthetic> = FxHashSet::default();
                Box::new(
                    items
                        .iter()
                        .flat_map(move |o| for_ob(**o))
                        .filter(move |e| seen.insert(*e)),
                )
            }
            SetFilter::Any(items) => Box::new(items.iter().flat_map(move |o| for_ob(**o))),
            SetFilter::All(items) => {
                if items.is_empty() {
                    // `SetFilter::check` is vacuously true for an empty `All`, i.e. it imposes
                    // no constraint, so every event of the type is a candidate, exactly as when
                    // `objs` itself is empty.
                    Box::new(EventOrSynthetic::get_all_syn_evs(linked_ocel, etype).into_iter())
                } else {
                    Box::new(for_ob(*items[0]).filter(|e| {
                        items
                            .iter()
                            .skip(1)
                            .all(|o| e.get_e2o_set(linked_ocel).contains(o))
                    }))
                }
            }
        }
    };
    initial.filter(|e| {
        let obs = &e.get_e2o_set(linked_ocel);
        for o in objs.iter() {
            if !o.check(obs) {
                return false;
            }
        }
        true
    })
}

fn directly_adjacent_event<'a>(
    objs: &'a [SetFilter<&'a ObjectIndex>],
    linked_ocel: &'a SlimLinkedOCEL,
    // reference_mock_event_index: &'a EventIndex,
    reference_time: &'a DateTime<FixedOffset>,
    // reference_event: &'a OCELEvent,
    following: bool,
) -> Option<EventOrSynthetic> {
    let in_direction = move |e: &EventOrSynthetic| {
        let e_time = e.get_timestamp(linked_ocel);
        if following {
            e_time > *reference_time
        } else {
            e_time < *reference_time
        }
    };
    let initial: Box<dyn Iterator<Item = EventOrSynthetic>> = if objs.is_empty() {
        // If no requirements are specified, consider all events
        // TODO: Maybe also consider synthetic events here?
        // But in general, this is not very relevant as there are usually some object requirements
        Box::new(
            linked_ocel
                .get_all_evs()
                .map(EventOrSynthetic::Event)
                .filter(in_direction),
        )
    } else {
        match &objs[0] {
            SetFilter::Any(items) => Box::new(items.iter().flat_map(|o| {
                EventOrSynthetic::get_all_for_ob(linked_ocel, **o)
                    .into_iter()
                    .filter(|e| {
                        let e_time = e.get_timestamp(linked_ocel);
                        if following {
                            e_time > *reference_time
                        } else {
                            e_time < *reference_time
                        }
                    })
            })),
            SetFilter::All(items) => {
                if items.is_empty() {
                    // `SetFilter::check` is vacuously true for an empty `All`: no constraint,
                    // so every event is a candidate, as when `objs` itself is empty.
                    Box::new(
                        linked_ocel
                            .get_all_evs()
                            .map(EventOrSynthetic::Event)
                            .filter(in_direction),
                    )
                } else {
                    Box::new(
                        EventOrSynthetic::get_all_for_ob(linked_ocel, *items[0])
                            .into_iter()
                            .filter(|e| {
                                let e_time = e.get_timestamp(linked_ocel);
                                if following {
                                    e_time > *reference_time
                                } else {
                                    e_time < *reference_time
                                }
                            }),
                    )
                }
            }
        }
    };
    let x = initial.filter(|e| {
        for o in objs.iter() {
            let obs = &e.get_e2o_set(linked_ocel);
            if !o.check(obs) {
                return false;
            }
        }
        true
    });
    match following {
        true => x.min_by_key(|a| a.get_timestamp(linked_ocel)),
        false => x.max_by_key(|a| a.get_timestamp(linked_ocel)),
    }
}

/// compute Score_time(e_s, e_t) between the source and target events, given the time interval constraint
fn calculate_pair_time_score(
    from_time: &DateTime<FixedOffset>,
    to_time: &DateTime<FixedOffset>,
    arc_type: OCDeclareArcType,
    interval: &Option<TimeInterval>,
) -> f64 {
    let Some(ref inter) = interval else {
        return 1.0;
    };
    let raw_duration = to_time.signed_duration_since(*from_time);

    let relative_duration = match arc_type {
        OCDeclareArcType::EF => raw_duration,
        OCDeclareArcType::EP => -raw_duration,
        OCDeclareArcType::AS => return 1.0,
        _ => return 1.0,
    };

    inter.evaluate_deviation(relative_duration)
}

/// compute fine-grained conformance score for a single activated event e_s: Score(e_s) = max_{e_t} (Score_obj * Score_time)
pub(crate) fn event_fine_grained_score(
    ev_index: &EventOrSynthetic,
    label: &OCDeclareArcLabel,
    to_et: &str,
    interval: &Option<TimeInterval>,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
    view: Option<E2ORevTypeView<'_>>,
) -> f64 {
    event_fine_grained_score_components(
        ev_index,
        label,
        to_et,
        interval,
        arc_type,
        linked_ocel,
        view,
    )
    .combined_score
}

/// Get fraction of source events violating this constraint arc
///
/// Returns a value from 0 (all source events satisfy this constraint) to 1 (all source events violate this constraint)
pub(crate) fn violation_fraction(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    arc_type: &OCDeclareArcType,
    counts: &(Option<usize>, Option<usize>),
    linked_ocel: &SlimLinkedOCEL,
    index: Option<&E2ORevByTypeIndex>,
) -> f64 {
    let view = index.and_then(|i| i.for_ev_type(to_et));
    let evs = EventOrSynthetic::get_all_syn_evs(linked_ocel, from_et);
    let ev_count = evs.len();
    let violated_evs_count = evs
        // .into_par_iter()
        .into_iter()
        .filter(|ev| event_violates(ev, label, to_et, arc_type, counts, linked_ocel, view))
        .count();
    violated_evs_count as f64 / ev_count as f64
}

/// Returns the time interval between the source and target events, based on the 5th and 80th percentiles of the positive time differences
pub fn discover_time_interval(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
) -> Option<TimeInterval> {
    let mut deltas_sec: Vec<f64> = Vec::new();

    // 1. collect all source and target events of the given event types
    let from_evs: Vec<_> = linked_ocel.get_evs_of_type(from_et).collect();
    //let to_evs: Vec<_> = linked_ocel.get_evs_of_type(to_et).collect();

    // 2. go through all pairs of source and target events, compute the time difference, and collect the positive differences
    for &e_s in &from_evs {
        // direct return DateTime
        let t_s = linked_ocel.get_ev_time(e_s);

        let source = EventOrSynthetic::Event(*e_s);

        // get object bindings for the source event
        let bindings: Vec<_> = label.get_bindings(&source, linked_ocel).collect();

        if bindings.is_empty() {
            continue;
        }

        // collect candidate target which satisfy min. 1 binding
        let mut candidates = std::collections::HashSet::new();

        for binding in &bindings {
            candidates.extend(target_events_for_binding(binding, linked_ocel, to_et, None));
        }

        // only keep fulfillment target which satisfy all bindings
        let full_candidates: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| {
                bindings.iter().all(|binding| {
                    target_events_for_binding(binding, linked_ocel, to_et, None)
                        .any(|target| target == *candidate)
                })
            })
            .collect();

        // EF：take earliest target
        // EP：take latest target
        let selected_target = match arc_type {
            OCDeclareArcType::EF => full_candidates
                .into_iter()
                .filter(|target| target.get_timestamp(linked_ocel) > *t_s)
                .min_by_key(|target| target.get_timestamp(linked_ocel)),

            OCDeclareArcType::EP => full_candidates
                .into_iter()
                .filter(|target| target.get_timestamp(linked_ocel) < *t_s)
                .max_by_key(|target| target.get_timestamp(linked_ocel)),

            OCDeclareArcType::AS => return None,
            _ => return None,
        };

        if let Some(target) = selected_target {
            let raw_diff = target
                .get_timestamp(linked_ocel)
                .signed_duration_since(*t_s)
                .num_seconds();

            let diff = match arc_type {
                OCDeclareArcType::EF => raw_diff,
                OCDeclareArcType::EP => -raw_diff,
                _ => unreachable!(),
            };

            if diff > 0 {
                deltas_sec.push(diff as f64);
            }
        }
        /*let objs_s: Vec<_> = linked_ocel
        .get_e2o(e_s)
        .map(|(_qualifier, obj_idx)| obj_idx)
        .collect();

        for &e_t in &to_evs {
        // check if the target event e_t shares any object with the source event e_s
        let share_object = linked_ocel
            .get_e2o(e_t)
            .any(|(_qualifier, obj_t)| objs_s.contains(&obj_t));

        if share_object {
            let t_t = linked_ocel.get_ev_time(e_t);
            let raw_diff = (*t_t - *t_s).num_seconds();

            let diff = match arc_type {
                OCDeclareArcType::EF => raw_diff,
                OCDeclareArcType::EP => -raw_diff,
                OCDeclareArcType::AS => return None,
                _ => return None,
            };

            if diff > 0 {
                deltas_sec.push(diff as f64);
            }
        }
        }*/
    }

    if deltas_sec.is_empty() {
        return None;
    }

    // 3. calculate the 5th and 80th percentiles of the positive time differences
    deltas_sec.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = deltas_sec.len();

    let p5_idx = ((n as f64 * 0.05).floor() as usize).min(n - 1);
    let p80_idx = ((n as f64 * 0.80).floor() as usize).min(n - 1);

    let min_sec = deltas_sec[p5_idx].max(0.0) as i64;
    let max_sec = deltas_sec[p80_idx].max(0.0) as i64;

    Some(TimeInterval {
        min_duration: Some(Duration::zero()),
        max_duration: Some(Duration::seconds(max_sec)),
    })
}

pub(crate) fn event_fine_grained_score_components(
    ev_index: &EventOrSynthetic,
    label: &OCDeclareArcLabel,
    to_et: &str,
    interval: &Option<TimeInterval>,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
    view: Option<E2ORevTypeView<'_>>,
) -> FineGrainedScores {
    let from_time = ev_index.get_timestamp(linked_ocel);
    let mut best_object_score: f64 = 0.0;
    let mut temporal_score_of_best_combined: f64 = 0.0;
    let mut best_combined_score: f64 = 0.0;
    let bindings: Vec<_> = label.get_bindings(ev_index, linked_ocel).collect();
    let total_bindings = bindings.len() as f64;

    if total_bindings == 0.0 {
        return FineGrainedScores {
            object_score: 1.0,
            temporal_score: 1.0,
            combined_score: 1.0,
        };
    }

    use std::collections::HashSet;
    let mut candidate_evs = HashSet::new();

    for binding in &bindings {
        for ev2 in target_events_for_binding(binding, linked_ocel, to_et, view) {
            candidate_evs.insert(ev2);
        }
    }

    for ev2 in candidate_evs {
        let to_time = ev2.get_timestamp(linked_ocel);

        let score_time =
            calculate_pair_time_score(&from_time, &to_time, arc_type, interval);

        let mut matched_count = 0.0;
        for binding in &bindings {
            let mut target_evs =
                target_events_for_binding(binding, linked_ocel, to_et, view);

            if target_evs.any(|e| e == ev2) {
                matched_count += 1.0;
            }
        }

        let score_obj = matched_count / total_bindings;
        let score_joint = score_obj * score_time;

        // best candidate at object level
        if score_obj > best_object_score {
            best_object_score = score_obj;
        }

        // obj x temporal and keep highest combined score
        if score_joint > best_combined_score {
            best_combined_score = score_joint;
            temporal_score_of_best_combined = score_time;
        }
    }

    FineGrainedScores {
        object_score: best_object_score,
        temporal_score: temporal_score_of_best_combined,
        combined_score: best_combined_score,
    }
}

//separate score
pub struct FineGrainedScores {
    pub object_score: f64,
    pub temporal_score: f64,
    pub combined_score: f64,
}


pub fn evaluate_arc_score_components(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
) -> FineGrainedScores {
    let interval = discover_time_interval(from_et, to_et, label, arc_type, linked_ocel);
    
    arc_fine_grained_score_components(
    from_et,
    to_et,
    label,
    &interval,
    arc_type,
    linked_ocel,
    None,
)
}

pub fn evaluate_arc_conformance(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
) -> f64 {
    evaluate_arc_score_components(from_et, to_et, label, arc_type, linked_ocel).combined_score
}

pub fn arc_fine_grained_score_components(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    interval: &Option<TimeInterval>,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
    index: Option<&E2ORevByTypeIndex>,
) -> FineGrainedScores {
    let view = index.and_then(|i| i.for_ev_type(to_et));
    let evs = EventOrSynthetic::get_all_syn_evs(linked_ocel, from_et);
    let ev_count = evs.len();

    if ev_count == 0 {
        return FineGrainedScores {
            object_score: 1.0,
            temporal_score: 1.0,
            combined_score: 1.0,
        };
    }

    let mut total_object_score = 0.0;
    let mut total_temporal_score = 0.0;
    let mut total_combined_score = 0.0;

    for ev in &evs {
        let scores = event_fine_grained_score_components(
            ev,
            label,
            to_et,
            interval,
            arc_type,
            linked_ocel,
            view,
        );

        total_object_score += scores.object_score;
        total_temporal_score += scores.temporal_score;
        total_combined_score += scores.combined_score;
    }

    FineGrainedScores {
        object_score: total_object_score / ev_count as f64,
        temporal_score: total_temporal_score / ev_count as f64,
        combined_score: total_combined_score / ev_count as f64,
    }
}


pub fn arc_fine_grained_conformance(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    interval: &Option<TimeInterval>,
    arc_type: OCDeclareArcType,
    linked_ocel: &SlimLinkedOCEL,
    index: Option<&E2ORevByTypeIndex>,
) -> f64 {
    arc_fine_grained_score_components(
        from_et,
        to_et,
        label,
        interval,
        arc_type,
        linked_ocel,
        index,
    ).combined_score
}

/// Checks whether the number of events violating this constraint arc is below (<=) the given noise threshold
///
/// Returns false, if the fraction of events violating the constraint is above the noise threshold.
#[allow(clippy::too_many_arguments)]
pub(crate) fn satisfies_threshold(
    from_et: &str,
    to_et: &str,
    label: &OCDeclareArcLabel,
    arc_type: &OCDeclareArcType,
    counts: &(Option<usize>, Option<usize>),
    linked_ocel: &SlimLinkedOCEL,
    violation_thresh: f64,
    index: Option<&E2ORevByTypeIndex>,
) -> bool {
    let view = index.and_then(|i| i.for_ev_type(to_et));
    let evs = EventOrSynthetic::get_all_syn_evs(linked_ocel, from_et);
    let ev_count = evs.len();
    let min_s = (ev_count as f64 * (1.0 - violation_thresh)).ceil() as u32;
    let min_v = (ev_count as f64 * violation_thresh).floor() as u32 + 1;
    // Non-Atomic:
    // for ev in evs {
    //     let violated = get_for_ev_perf(&ev, label, to_et, arc_type, counts, linked_ocel);
    //     if violated {
    //         min_v -= 1;
    //         if min_v == 0 {
    //             return false;
    //         }
    //     } else {
    //         min_s -= 1;
    //         if min_s == 0 {
    //             return true;
    //         }
    //     }
    // }
    // if min_s <= 0 {
    //     return true;
    // }
    // if min_v <= 0 {
    //     return false;
    // }

    // // Atomic:
    let min_v_atomic = AtomicU32::new(0);
    let min_s_atomic = AtomicU32::new(0);
    evs.into_par_iter()
        .map(|ev| {
            let violated = event_violates(&ev, label, to_et, arc_type, counts, linked_ocel, view);
            if violated {
                min_v_atomic.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                min_s_atomic.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .take_any_while(|_| {
            if min_s_atomic.load(std::sync::atomic::Ordering::Relaxed) >= min_s {
                return false;
            }
            if min_v_atomic.load(std::sync::atomic::Ordering::Relaxed) >= min_v {
                return false;
            }
            true
        })
        .for_each(|_| {});
    let min_s_atomic = min_s_atomic.into_inner();
    let min_v_atomic = min_v_atomic.into_inner();
    // println!("{} and {}",min_s_atomic,min_v_atomic);
    if min_s_atomic >= min_s {
        return true;
    }
    if min_v_atomic >= min_v {
        return false;
    }

    unreachable!()
}

/// Returns true if violated!
#[allow(clippy::too_many_arguments)]
pub(crate) fn event_violates(
    ev_index: &EventOrSynthetic,
    label: &OCDeclareArcLabel,
    to_et: &str,
    arc_type: &OCDeclareArcType,
    counts: &(Option<usize>, Option<usize>),
    linked_ocel: &SlimLinkedOCEL,
    view: Option<E2ORevTypeView<'_>>,
) -> bool {
    let syn_time = ev_index.get_timestamp(linked_ocel);
    label.get_bindings(ev_index, linked_ocel).any(|binding| {
        match arc_type {
            OCDeclareArcType::AS | OCDeclareArcType::EF | OCDeclareArcType::EP => {
                let target_ev_iterator =
                    target_events_for_binding(&binding, linked_ocel, to_et, view).filter(|ev2| {
                        let ev2_time = ev2.get_timestamp(linked_ocel);
                        match arc_type {
                            OCDeclareArcType::EF => syn_time < ev2_time,
                            OCDeclareArcType::EP => syn_time > ev2_time,
                            OCDeclareArcType::AS => true,
                            _ => unreachable!("DF should not go here."),
                        }
                    });
                if counts.1.is_none() {
                    // Only take necessary
                    if counts.0.unwrap_or_default()
                        > target_ev_iterator
                            .take(counts.0.unwrap_or_default())
                            .count()
                    {
                        // Violated!
                        return true;
                    }
                } else if let Some(c) = counts.1 {
                    let count = target_ev_iterator.take(c + 1).count();
                    if c < count || count < counts.0.unwrap_or_default() {
                        // Violated
                        return true;
                    }
                }
                false
            }
            OCDeclareArcType::DF | OCDeclareArcType::DP => {
                let df_ev = directly_adjacent_event(
                    &binding,
                    linked_ocel,
                    &syn_time,
                    arc_type == &OCDeclareArcType::DF,
                );
                let count = if df_ev.is_some_and(|e| e.get_as_event_type(linked_ocel) == to_et) {
                    1
                } else {
                    0
                };
                if let Some(min_c) = counts.0 {
                    if count < min_c {
                        return true;
                    }
                }
                if let Some(max_c) = counts.1 {
                    if count > max_c {
                        return true;
                    }
                }
                false
            }
        }
    })
}

#[register_binding]
/// Returns the confidence conformance of an OC-DECLARE arc on the given OCEL
///
/// Returns a value from 0.0 (all source events violate this constraint) to 1.0 (all source events satisfy this constraint)
pub fn oc_declare_conformance(ocel: &SlimLinkedOCEL, arc: &OCDeclareArc) -> f64 {
    1.0 - violation_fraction(
        arc.from.as_str(),
        arc.to.as_str(),
        &arc.label,
        &arc.arc_type,
        &arc.counts,
        ocel,
        None,
    )
}

#[cfg(test)]

mod tests {
    use super::*;
    use crate::core::event_data::object_centric::{
        appendable::AppendableOCEL, OCELRelationship, OCELType,
    };
    use crate::core::process_models::oc_declare::ObjectTypeAssociation;
    use chrono::DateTime;

    fn empty_type(name: &str) -> OCELType {
        OCELType {
            name: name.into(),
            attributes: Vec::new(),
        }
    }

    /// Small OCEL where EACH/ANY/ALL involvements all have something to check
    fn sample_locel() -> SlimLinkedOCEL {
        let mut s = SlimLinkedOCEL::new();
        for t in ["place", "pay", "ship"] {
            s.declare_event_type(empty_type(t)).unwrap();
        }
        for t in ["order", "item"] {
            s.declare_object_type(empty_type(t)).unwrap();
        }
        for (id, ot) in [
            ("o1", "order"),
            ("o2", "order"),
            ("i1", "item"),
            ("i2", "item"),
        ] {
            s.append_object(id.into(), ot, Vec::new(), Vec::new())
                .unwrap();
        }
        let evs = [
            ("e0", "place", vec!["o1", "i1"]),
            ("e1", "place", vec!["o2", "i2"]),
            ("e2", "pay", vec!["o1"]),
            ("e3", "ship", vec!["o1", "i1"]),
            ("e4", "pay", vec!["o2"]),
            ("e5", "place", vec!["o1", "i2"]),
            ("e6", "ship", vec!["o2", "i2"]),
        ];
        for (i, (id, et, obs)) in evs.iter().enumerate() {
            let time =
                DateTime::parse_from_rfc3339(&format!("2024-01-0{}T00:00:00Z", i + 1)).unwrap();
            s.append_event(
                (*id).into(),
                et,
                time,
                Vec::new(),
                obs.iter()
                    .map(|o| OCELRelationship {
                        object_id: (*o).into(),
                        qualifier: "q".into(),
                    })
                    .collect(),
            )
            .unwrap();
        }
        s.finalize().unwrap();
        s
    }

    /// Checking with and without the index must agree, across arc types/labels/counts
    #[test]
    fn constraint_checks_agree_with_and_without_index() {
        let locel = sample_locel();
        let index = E2ORevByTypeIndex::build(&locel);
        let assoc = ObjectTypeAssociation::new_simple("order");
        let item = ObjectTypeAssociation::new_simple("item");
        let labels = [
            OCDeclareArcLabel {
                each: vec![assoc.clone()],
                any: vec![],
                all: vec![],
            },
            OCDeclareArcLabel {
                each: vec![],
                any: vec![assoc.clone()],
                all: vec![],
            },
            OCDeclareArcLabel {
                each: vec![],
                any: vec![],
                all: vec![assoc.clone()],
            },
            OCDeclareArcLabel {
                each: vec![item],
                any: vec![assoc],
                all: vec![],
            },
            OCDeclareArcLabel {
                each: vec![],
                any: vec![],
                all: vec![],
            },
        ];
        let arc_types = [
            OCDeclareArcType::AS,
            OCDeclareArcType::EF,
            OCDeclareArcType::EP,
            OCDeclareArcType::DF,
            OCDeclareArcType::DP,
        ];
        let counts = [
            (Some(1), None),
            (Some(1), Some(1)),
            (Some(0), Some(2)),
            (Some(2), None),
        ];
        let mut compared = 0;
        for from in locel.get_ev_types().map(str::to_string).collect::<Vec<_>>() {
            for to in locel.get_ev_types().map(str::to_string).collect::<Vec<_>>() {
                for label in &labels {
                    for at in &arc_types {
                        for c in &counts {
                            let scanned =
                                violation_fraction(&from, &to, label, at, c, &locel, None);
                            let indexed =
                                violation_fraction(&from, &to, label, at, c, &locel, Some(&index));
                            assert_eq!(
                                scanned, indexed,
                                "{from} -> {to}, {at:?}, {label:?}, {c:?}"
                            );
                            for thresh in [0.0, 0.2, 1.0] {
                                let p = satisfies_threshold(
                                    &from, &to, label, at, c, &locel, thresh, None,
                                );
                                let i = satisfies_threshold(
                                    &from,
                                    &to,
                                    label,
                                    at,
                                    c,
                                    &locel,
                                    thresh,
                                    Some(&index),
                                );
                                assert_eq!(p, i, "threshold {thresh}: {from} -> {to}, {at:?}");
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert!(
            compared > 500,
            "expected at least 500 comparison instead of just {compared}; something is wrong with the input SlimLinked OCEL."
        );
    }

    // `SetFilter::check` is vacuously true for `All(&[])`, since an event trivially references
    // "all of" an empty set, so the candidate iterator must not special-case it to empty.
    #[test]
    fn all_of_empty_items_matches_every_event_of_the_type_like_no_objs_at_all() {
        let locel = sample_locel();
        let no_objs: Vec<SetFilter<&ObjectIndex>> = Vec::new();
        let empty_all: Vec<SetFilter<&ObjectIndex>> = vec![SetFilter::All(Vec::new())];

        let via_no_objs: FxHashSet<_> =
            target_events_for_binding(&no_objs, &locel, "place", None).collect();
        let via_empty_all: FxHashSet<_> =
            target_events_for_binding(&empty_all, &locel, "place", None).collect();

        assert_eq!(via_empty_all, via_no_objs);
        assert_eq!(via_empty_all.len(), locel.get_evs_of_type("place").count());
    }
    #[test]
    fn test_fine_grained_conformance_with_sample_locel() {
        use chrono::Duration;

        let locel = sample_locel();
        let index = E2ORevByTypeIndex::build(&locel);

        // 1. create a label with EACH association to "item"
        let item_assoc = ObjectTypeAssociation::new_simple("item");
        let label = OCDeclareArcLabel {
            each: vec![item_assoc],
            any: vec![],
            all: vec![],
        };

        // 2. time interval: min_duration = 0 days, max_duration = 2 days
        let interval = Some(TimeInterval {
            min_duration: Some(Duration::days(0)),
            max_duration: Some(Duration::days(2)),
        });

        // 3. fine-grained conformance score for "place" -> "ship" with the label and interval
        let score =
            arc_fine_grained_conformance("place", "ship", &label, &interval, &locel, Some(&index));

        println!("\n==========================================");
        println!("Calculated Fine-grained Score: {:.4}", score);
        println!("==========================================\n");

        assert!(score > 0.0 && score <= 1.0, "Score should be in (0, 1]");
    }
}
