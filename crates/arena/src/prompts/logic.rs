//! Pure prompt logic (no ECS): default assignments, validity, answer JSON. Unit-tested because a
//! wrong answer shape stalls the engine, which is far worse than an ugly dialog.

use std::collections::BTreeMap;

use mtg_protocol::{Card, ChoiceOption};
use serde_json::{Value, json};

/// Key used for the defending player/planeswalker in damage assignment (`"defender"` on the wire).
pub const DEFENDER: &str = "defender";

pub fn has_keyword(card: &Card, kw: &str) -> bool {
    card.state.as_ref().is_some_and(|s| s.keywords.iter().any(|k| k.to_lowercase().contains(kw)))
}

/// Damage still needed to kill `b` (deathtouch needs 1). Hidden/unknown blockers need everything.
pub fn lethal_for(attacker: &Card, b: &Card, remaining: i64) -> i64 {
    if b.hidden || b.state.is_none() {
        return remaining.max(1);
    }
    if has_keyword(attacker, "deathtouch") {
        return 1;
    }
    let t = b.state.as_ref().and_then(|s| s.toughness).unwrap_or(1) as i64;
    (t - b.damage as i64).max(1)
}

/// Lethal damage to each blocker in order; the remainder goes to the defender when the attacker
/// may trample over (defender present and trample/no blockers), else to the last blocker.
pub fn default_damage(
    attacker: &Card,
    blockers: &[Card],
    damage: i64,
    has_defender: bool,
) -> BTreeMap<String, i64> {
    let mut v: BTreeMap<String, i64> = blockers.iter().map(|b| (b.id.to_string(), 0)).collect();
    if has_defender {
        v.insert(DEFENDER.into(), 0);
    }
    let mut left = damage;
    for b in blockers {
        if left <= 0 {
            break;
        }
        let give = left.min(lethal_for(attacker, b, left));
        v.insert(b.id.to_string(), give);
        left -= give;
    }
    if left > 0 {
        if has_defender && (has_keyword(attacker, "trample") || blockers.is_empty()) {
            *v.entry(DEFENDER.into()).or_insert(0) += left;
        } else if let Some(last) = blockers.last() {
            *v.entry(last.id.to_string()).or_insert(0) += left;
        } else if has_defender {
            *v.entry(DEFENDER.into()).or_insert(0) += left;
        }
    }
    v
}

pub fn total(v: &BTreeMap<String, i64>) -> i64 {
    v.values().sum()
}

/// Adjust `key` by `delta`, never below 0 and never pushing the total above `cap` (if any).
pub fn step_value(v: &mut BTreeMap<String, i64>, key: &str, delta: i64, cap: Option<i64>) {
    let cur = v.get(key).copied().unwrap_or(0);
    let mut next = (cur + delta).max(0);
    if let Some(cap) = cap {
        let others = total(v) - cur;
        next = next.min((cap - others).max(0));
    }
    v.insert(key.to_string(), next);
}

/// Every blocker key is sent (zeros included, like the React client the engine was soaked with);
/// `defender` only when it takes damage.
pub fn damage_answer(v: &BTreeMap<String, i64>) -> Value {
    let mut m = serde_json::Map::new();
    for (k, n) in v {
        if k != DEFENDER || *n > 0 {
            m.insert(k.clone(), json!(n));
        }
    }
    Value::Object(m)
}

/// `{ "<i>": n }` for assignAmount.
pub fn amount_answer(v: &BTreeMap<String, i64>) -> Value {
    let mut m = serde_json::Map::new();
    for (k, n) in v {
        m.insert(k.clone(), json!(n));
    }
    Value::Object(m)
}

pub fn amount_valid(v: &BTreeMap<String, i64>, amount: i64, at_least_one: bool) -> bool {
    total(v) == amount && (!at_least_one || v.values().all(|n| *n >= 1))
}

/// (min, max) for a pick prompt; negative on the wire = informational list.
pub fn pick_bounds(min: i64, max: i64, n_options: usize) -> (bool, i64, i64) {
    let informational = min < 0 && max < 0;
    let lo = min.max(0);
    let hi = if max < 0 { n_options as i64 } else { max };
    (informational, lo, hi)
}

pub fn pick_valid(sel: &[i64], lo: i64, hi: i64) -> bool {
    (sel.len() as i64) >= lo && (sel.len() as i64) <= hi
}

pub fn toggle_pick(sel: &mut Vec<i64>, i: i64, hi: i64) {
    if let Some(p) = sel.iter().position(|x| *x == i) {
        sel.remove(p);
    } else if hi == 1 {
        sel.clear();
        sel.push(i);
    } else if (sel.len() as i64) < hi {
        sel.push(i);
    }
}

pub fn order_valid(src_len: usize, remain_min: i64, remain_max: i64) -> bool {
    (src_len as i64) >= remain_min && (remain_max < 0 || (src_len as i64) <= remain_max)
}

pub fn order_answer(dst: &[i64], remember: bool) -> Value {
    json!({ "order": dst, "remember": remember })
}

pub fn split_order(options: &[ChoiceOption], source_count: i64) -> (Vec<i64>, Vec<i64>) {
    let n = source_count.max(0) as usize;
    (options.iter().take(n).map(|o| o.i).collect(), options.iter().skip(n).map(|o| o.i).collect())
}

/// Swap helper for reorder lists; returns false when out of range.
pub fn shift<T>(v: &mut [T], idx: usize, dir: i32) -> bool {
    let j = idx as i64 + dir as i64;
    if j < 0 || j as usize >= v.len() || idx >= v.len() {
        return false;
    }
    v.swap(idx, j as usize);
    true
}

/// Move item to the start (top) or end (bottom) of the list.
pub fn move_edge<T>(v: &mut Vec<T>, idx: usize, top: bool) {
    if idx >= v.len() {
        return;
    }
    let item = v.remove(idx);
    if top {
        v.insert(0, item);
    } else {
        v.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtg_protocol::CardState;

    fn creature(id: i64, t: i32, dmg: i32, kws: &[&str]) -> Card {
        Card {
            id,
            damage: dmg,
            state: Some(CardState {
                toughness: Some(t),
                power: Some(2),
                keywords: kws.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn lethal_first_then_remainder_on_last_blocker() {
        let a = creature(1, 5, 0, &[]);
        let bl = vec![creature(2, 2, 0, &[]), creature(3, 3, 1, &[])];
        let v = default_damage(&a, &bl, 6, false);
        assert_eq!(v["2"], 2);
        assert_eq!(v["3"], 4); // 2 lethal + 2 spare
        assert_eq!(total(&v), 6);
    }

    #[test]
    fn trample_remainder_goes_to_defender() {
        let a = creature(1, 5, 0, &["Trample"]);
        let bl = vec![creature(2, 2, 0, &[])];
        let v = default_damage(&a, &bl, 5, true);
        assert_eq!(v["2"], 2);
        assert_eq!(v[DEFENDER], 3);
    }

    #[test]
    fn deathtouch_needs_one() {
        let a = creature(1, 5, 0, &["Deathtouch"]);
        let bl = vec![creature(2, 9, 0, &[]), creature(3, 9, 0, &[])];
        let v = default_damage(&a, &bl, 2, false);
        assert_eq!((v["2"], v["3"]), (1, 1));
    }

    #[test]
    fn step_respects_cap_and_floor() {
        let mut v = BTreeMap::from([("a".to_string(), 2), ("b".to_string(), 1)]);
        step_value(&mut v, "a", 5, Some(4));
        assert_eq!(v["a"], 3);
        step_value(&mut v, "b", -9, Some(4));
        assert_eq!(v["b"], 0);
    }

    #[test]
    fn damage_answer_keeps_blockers_drops_idle_defender() {
        let v = BTreeMap::from([("2".to_string(), 3), ("3".to_string(), 0), (DEFENDER.to_string(), 0)]);
        assert_eq!(damage_answer(&v), json!({"2": 3, "3": 0}));
    }

    #[test]
    fn amount_rules() {
        let v = BTreeMap::from([("0".to_string(), 2), ("1".to_string(), 0)]);
        assert!(amount_valid(&v, 2, false));
        assert!(!amount_valid(&v, 2, true));
    }

    #[test]
    fn pick_toggle_single_and_multi() {
        let mut s = vec![];
        toggle_pick(&mut s, 3, 1);
        toggle_pick(&mut s, 4, 1);
        assert_eq!(s, vec![4]);
        let mut m = vec![];
        for i in 0..5 {
            toggle_pick(&mut m, i, 2);
        }
        assert_eq!(m, vec![0, 1]);
        assert_eq!(pick_bounds(-1, -1, 4), (true, 0, 4));
    }

    #[test]
    fn order_helpers() {
        let mut v = vec![1, 2, 3];
        assert!(shift(&mut v, 0, 1));
        assert!(!shift(&mut v, 2, 1));
        assert_eq!(v, vec![2, 1, 3]);
        move_edge(&mut v, 2, true);
        assert_eq!(v, vec![3, 2, 1]);
        assert!(order_valid(1, 0, 2));
        assert!(!order_valid(3, 0, 2));
        assert_eq!(order_answer(&[1, 2], true), json!({"order":[1,2],"remember":true}));
    }
}
