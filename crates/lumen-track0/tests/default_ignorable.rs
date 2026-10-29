//! The `Default_Ignorable_Code_Point` table against the pinned Unicode data.
//!
//! `data/default-ignorable-16.0.0.txt` holds the property's lines from the pinned
//! `DerivedCoreProperties.txt` unchanged (`scripts/gen-default-ignorable.py`
//! checks that against the full file and its SHA-256). This test reads those
//! lines and compares them with `is_default_ignorable` for every code point.

use lumen_track0::is_default_ignorable;

const DATA: &str = include_str!("../data/default-ignorable-16.0.0.txt");

/// The code points the data file lists, as inclusive ranges.
fn listed() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for line in DATA
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (range, rest) = line
            .split_once(';')
            .unwrap_or_else(|| unreachable!("a data line has a ';': {line}"));
        assert!(
            rest.trim_start()
                .starts_with("Default_Ignorable_Code_Point"),
            "{line}"
        );
        let (lo, hi) = range
            .trim()
            .split_once("..")
            .unwrap_or((range.trim(), range.trim()));
        let parse =
            |h: &str| u32::from_str_radix(h, 16).unwrap_or_else(|_| unreachable!("hex: {line}"));
        out.push((parse(lo), parse(hi)));
    }
    out
}

#[test]
fn the_data_file_names_its_source_and_digest() {
    assert!(DATA.contains("DerivedCoreProperties-16.0.0.txt"));
    assert!(DATA.contains("https://www.unicode.org/Public/16.0.0/ucd/DerivedCoreProperties.txt"));
    assert!(
        DATA.contains("39d35161f2954497f69e08bdb9e701493f476a3d30222de20028feda36c1dabd"),
        "the digest of the pinned file"
    );
}

#[test]
fn the_table_agrees_with_the_unicode_data_for_every_code_point() {
    let ranges = listed();
    assert_eq!(
        ranges.len(),
        27,
        "the pinned file lists 27 lines for the property"
    );
    let mut count = 0_u32;
    for cp in 0..=0x10_ffff_u32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let want = ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp));
        assert_eq!(is_default_ignorable(c), want, "U+{cp:04X}");
        count += u32::from(want);
    }
    // 4174 code points in Unicode 16: a second check on the data itself.
    let listed_total: u32 = ranges.iter().map(|&(lo, hi)| hi - lo + 1).sum();
    assert_eq!(count, listed_total);
    assert_eq!(
        count, 4174,
        "the number of Default_Ignorable_Code_Point code points in Unicode 16"
    );
}

#[test]
fn the_neighbours_of_every_range_are_not_ignorable() {
    for (lo, hi) in listed() {
        for cp in [lo.wrapping_sub(1), hi + 1] {
            let in_other_range = listed().iter().any(|&(a, b)| (a..=b).contains(&cp));
            if let Some(c) = char::from_u32(cp)
                && !in_other_range
            {
                assert!(
                    !is_default_ignorable(c),
                    "U+{cp:04X} is next to a range but not in it"
                );
            }
        }
    }
}
