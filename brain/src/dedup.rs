//! Duplicates: exact copies by content hash, near-copies (`v1`, `v2`, `final_final`) by SimHash, a
//! 64-bit fingerprint in which two texts that share most of their words differ in few bits.
//! Of each group of copies the newest is the one that ranks; the others are its `versions`.

use std::collections::{HashMap, HashSet};

/// Shorter texts are too few words for a fingerprint to mean anything; they only match exactly.
const MIN_WORDS: usize = 40;
/// Words per feature: the fingerprint follows phrases, not a bag of words.
const SHINGLE: usize = 2;
/// Two texts this many bits apart or fewer are copies of each other.
pub const MAX_DISTANCE: u32 = 7;
/// A bucket this large holds boilerplate that every file shares; it says nothing about copies.
const MAX_BUCKET: usize = 5000;

/// FNV-1a, then the finalizer of MurmurHash3, so that every output bit depends on every input byte.
fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// The fingerprint of a text, or `None` when it is too short to have one.
pub fn simhash(text: &str) -> Option<u64> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.len() < MIN_WORDS {
        return None;
    }
    // Each phrase votes once, however often it occurs: a template's boilerplate, or a table's
    // repeated labels, must not outweigh what makes the text its own.
    let mut seen = HashSet::new();
    let mut votes = [0i32; 64];
    for gram in words.windows(SHINGLE) {
        let h = hash(gram.join(" ").as_bytes());
        if !seen.insert(h) {
            continue;
        }
        for (bit, vote) in votes.iter_mut().enumerate() {
            *vote += if h >> bit & 1 == 1 { 1 } else { -1 };
        }
    }
    Some(
        votes
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > 0)
            .fold(0u64, |acc, (bit, _)| acc | 1 << bit),
    )
}

/// A file as the grouping sees it.
pub struct Item {
    pub path: String,
    /// Content hash of the file.
    pub hash: String,
    pub date: String,
    pub fingerprint: Option<u64>,
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Each file that is a copy of a newer one, with the path of the newest of its group. A tie in
/// date goes to the path that sorts last, so the answer does not depend on the order of `items`.
pub fn copies(items: &[Item]) -> Vec<(String, String)> {
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let union = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    };
    let mut by_hash: HashMap<&str, usize> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(&first) = by_hash.get(item.hash.as_str()) {
            union(&mut parent, first, i);
        } else {
            by_hash.insert(&item.hash, i);
        }
    }
    // Texts at most MAX_DISTANCE bits apart agree on at least one of MAX_DISTANCE + 1 slices of
    // the fingerprint, so only files that share a slice are compared.
    let slices = MAX_DISTANCE as usize + 1;
    let mut buckets: HashMap<(usize, u64), Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(fp) = item.fingerprint else { continue };
        for s in 0..slices {
            let (from, to) = (64 * s / slices, 64 * (s + 1) / slices);
            let key = fp >> from & ((1u64 << (to - from)) - 1);
            buckets.entry((s, key)).or_default().push(i);
        }
    }
    for bucket in buckets
        .values()
        .filter(|b| b.len() > 1 && b.len() <= MAX_BUCKET)
    {
        for (n, &a) in bucket.iter().enumerate() {
            for &b in &bucket[n + 1..] {
                if let (Some(x), Some(y)) = (items[a].fingerprint, items[b].fingerprint) {
                    if (x ^ y).count_ones() <= MAX_DISTANCE {
                        union(&mut parent, a, b);
                    }
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..items.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out = Vec::new();
    for members in groups.values().filter(|m| m.len() > 1) {
        let newest = members
            .iter()
            .max_by(|&&a, &&b| {
                (&items[a].date, &items[a].path).cmp(&(&items[b].date, &items[b].path))
            })
            .copied()
            .unwrap_or(members[0]);
        for &m in members.iter().filter(|&&m| m != newest) {
            out.push((items[m].path.clone(), items[newest].path.clone()));
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = "Staff are entitled to twenty five days of paid leave each calendar year in addition to public holidays. \
        Leave is agreed with the team lead at least two weeks ahead, and no more than ten working days may be taken in a row without \
        approval from the head of department. Unused days carry over until the end of May the following year, after which they expire. \
        Sick leave is recorded separately and does not reduce the annual entitlement, but a doctor's note is needed after three days. \
        Parental leave follows the statutory rules and is described in the separate parental leave policy, which is available from \
        the people team. Requests made during the summer months are decided by seniority when two people ask for the same week.";

    fn item(path: &str, hash: &str, date: &str, text: &str) -> Item {
        Item {
            path: path.into(),
            hash: hash.into(),
            date: date.into(),
            fingerprint: simhash(text),
        }
    }

    #[test]
    fn a_small_edit_moves_a_few_bits_and_another_text_many() {
        let a = simhash(POLICY).unwrap();
        let edited = POLICY
            .replace("twenty five", "thirty")
            .replace("two weeks", "three weeks");
        let b = simhash(&edited).unwrap();
        assert!(
            (a ^ b).count_ones() <= MAX_DISTANCE,
            "{}",
            (a ^ b).count_ones()
        );
        let other = simhash(
            "The office is closed on public holidays and during the week between Christmas and New Year, \
             when the building is locked at six in the evening. Visitors sign in at the front desk and receive a badge \
             which must be returned when they leave. Deliveries are accepted between nine and eleven in the morning only, \
             and parcels for staff are kept in the post room until they are collected by the person named on the label.",
        )
        .unwrap();
        assert!(
            (a ^ other).count_ones() > 15,
            "{}",
            (a ^ other).count_ones()
        );
        assert_eq!(simhash("too short"), None);
        assert_eq!(
            simhash(POLICY),
            simhash(&POLICY.to_uppercase()),
            "case and punctuation do not matter"
        );
    }

    #[test]
    fn the_newest_of_a_group_of_versions_is_the_one_that_stays() {
        let v2 = POLICY.replace("twenty five", "thirty");
        let fin = POLICY
            .replace("twenty five", "thirty")
            .replace("May", "June");
        let items = [
            item("leave_v1.docx", "h1", "2023-01-10", POLICY),
            item("leave_v2.docx", "h2", "2023-06-01", &v2),
            item("leave_final_final.docx", "h3", "2024-02-02", &fin),
            item(
                "unrelated.md",
                "h4",
                "2024-03-03",
                "something else entirely",
            ),
        ];
        assert_eq!(
            copies(&items),
            [
                (
                    "leave_v1.docx".to_string(),
                    "leave_final_final.docx".to_string()
                ),
                (
                    "leave_v2.docx".to_string(),
                    "leave_final_final.docx".to_string()
                ),
            ]
        );
    }

    #[test]
    fn exact_copies_match_by_hash_even_when_short_and_a_tie_goes_to_the_later_path() {
        let items = [
            item("b/copy.md", "same", "2024-01-01", "short"),
            item("a/original.md", "same", "2024-01-01", "short"),
            item("c/other.md", "different", "2024-01-01", "short"),
        ];
        assert_eq!(
            copies(&items),
            [("a/original.md".to_string(), "b/copy.md".to_string())]
        );
    }

    #[test]
    fn many_files_with_nothing_in_common_have_no_copies() {
        let items: Vec<Item> = (0..300)
            .map(|i| {
                let text: String = (0..60).map(|j| format!("w{}x{} ", i, j * 7 + i)).collect();
                item(&format!("f{i}.md"), &format!("h{i}"), "2024-01-01", &text)
            })
            .collect();
        assert!(copies(&items).is_empty());
    }

    #[test]
    fn a_shared_boilerplate_does_not_make_two_texts_copies() {
        let boilerplate = "Lorem ipsum dolor sit amet consectetur adipiscing elit pellentesque ultricies quam ut massa luctus cursus. ".repeat(40);
        let own = |tag: &str| (0..80).map(|i| format!("{tag}{i} ")).collect::<String>();
        let a = simhash(&format!("{boilerplate}{}", own("alpha"))).unwrap();
        let b = simhash(&format!("{boilerplate}{}", own("beta"))).unwrap();
        assert!(
            (a ^ b).count_ones() > MAX_DISTANCE,
            "{}",
            (a ^ b).count_ones()
        );
    }
}
