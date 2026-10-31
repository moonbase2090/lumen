//! Link targets: the allowlist, the full decoded target, and mailto.

#![cfg(feature = "links-clipboard")]

use lumen_term::link::{
    Checked, Kind, MAX_TARGET, Refusal, Verdict, check, copy_plain, copy_target,
};

fn confirm(target: &str, visible: &str) -> Checked {
    match check(target, visible) {
        Verdict::Confirm(c) => c,
        other => unreachable!("{target} should need a confirmation: {other:?}"),
    }
}

fn refused(target: &str) -> Refusal {
    match check(target, "text") {
        Verdict::Refused(why) => why,
        other => unreachable!("{target} should be refused: {other:?}"),
    }
}

#[test]
fn only_https_http_and_mailto_are_acted_on_and_every_other_scheme_is_refused() {
    assert_eq!(confirm("https://example.com/a", "x").kind, Kind::Web);
    assert_eq!(confirm("http://example.com", "x").kind, Kind::Web);
    assert_eq!(confirm("mailto:a@lumenprotocol.org", "x").kind, Kind::Mail);
    for target in [
        "ftp://example.com/",
        "javascript:alert(1)",
        "data:text/html,hi",
        "vbscript:x",
        "blob:https://example.com/x",
        "tel:+15551234",
        "sms:+15551234",
        "ssh://host/",
        "x-custom-handler:open",
        "about:blank",
        "example.com/no-scheme",
        "//example.com/x",
        ":x",
        "1http://example.com",
    ] {
        assert_eq!(refused(target), Refusal::Scheme, "{target}");
    }
}

#[test]
fn the_scheme_and_host_are_lower_case_in_what_is_shown() {
    let c = confirm("HTTPS://Example.COM/Path?Q=1", "x");
    assert_eq!(c.shown, "https://example.com/Path?Q=1");
    assert_eq!(c.host.as_deref(), Some("example.com"));
}

#[test]
fn a_file_target_is_inert_text_with_a_path_to_copy() {
    assert_eq!(
        check("file:///tmp/a%20b.txt", "x"),
        Verdict::Inert {
            path: "/tmp/a b.txt".into()
        }
    );
    assert_eq!(
        check("file://somehost/etc/hosts", "x"),
        Verdict::Inert {
            path: "/etc/hosts".into()
        },
        "a host in a file target is not interpreted"
    );
    assert_eq!(refused("file:///tmp/%0Aevil"), Refusal::Encoding);
}

#[test]
fn a_non_ascii_host_is_shown_in_punycode_and_flagged() {
    for (target, shown) in [
        (
            "https://b\u{fc}cher.example/",
            "https://xn--bcher-kva.example/",
        ),
        ("https://M\u{fc}nchen.de/", "https://xn--mnchen-3ya.de/"),
        (
            "https://\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}.\u{440}\u{444}/",
            "https://xn--e1afmkfd.xn--p1ai/",
        ),
        // The Cyrillic lookalike of "apple": not the Latin name it resembles.
        (
            "https://\u{430}\u{440}\u{440}\u{4cf}\u{435}.com/",
            "https://xn--80ak6aa92e.com/",
        ),
    ] {
        let c = confirm(target, "x");
        assert_eq!(c.shown, shown, "{target}");
        assert!(c.idn, "{target}");
        assert!(
            c.shown.is_ascii(),
            "the confirmation shows only ASCII: {}",
            c.shown
        );
    }
    assert!(
        confirm("https://xn--bcher-kva.example/", "x").idn,
        "an ASCII punycode label is flagged too"
    );
    assert!(!confirm("https://example.com/", "x").idn);
}

#[test]
fn the_host_is_never_cut_and_an_over_long_target_is_refused_not_cut() {
    let host = format!("{0}.{0}.{0}.{0}", "a".repeat(60));
    assert!(host.len() <= 253);
    let c = confirm(&format!("https://{host}/"), "x");
    assert_eq!(c.host.as_deref(), Some(host.as_str()));
    assert!(c.shown.contains(&host));
    let too_long = format!("https://{0}.{0}.{0}.{0}.{0}/", "a".repeat(60));
    assert_eq!(refused(&too_long), Refusal::Host);
    let huge = format!("https://example.com/{}", "a".repeat(MAX_TARGET));
    assert_eq!(refused(&huge), Refusal::Size);
    assert_eq!(refused(""), Refusal::Size);
}

#[test]
fn a_user_name_in_a_web_target_is_refused() {
    assert_eq!(
        refused(&format!("https://good.example{}evil.example/", '@')),
        Refusal::UserInfo
    );
    assert_eq!(
        refused("https://user:pw@lumenprotocol.org/"),
        Refusal::UserInfo
    );
}

#[test]
fn a_target_that_does_not_decode_cleanly_is_refused() {
    assert_eq!(refused("https://example.com/%zz"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/%"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/%4"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/%FF"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/%00"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/a%0Ab"), Refusal::Encoding);
    assert_eq!(refused("https://example.com/%E2%80%AE"), Refusal::Invisible);
    assert_eq!(
        refused("https://example.com/?q=%E2%80%8B"),
        Refusal::Invisible
    );
    // Well formed escapes of ordinary text pass.
    let c = confirm("https://example.com/%E2%9C%93?q=a%20b#f", "x");
    assert_eq!(c.shown, "https://example.com/%E2%9C%93?q=a%20b#f");
}

#[test]
fn control_space_and_invisible_characters_anywhere_are_refused() {
    assert_eq!(refused("https://example.com/a b"), Refusal::Control);
    assert_eq!(refused("https://example.com/a\nb"), Refusal::Control);
    assert_eq!(refused("https://example.com/a\u{1b}b"), Refusal::Control);
    assert_eq!(refused("https://example.com/a\u{85}b"), Refusal::Control);
    assert_eq!(refused("https://exa\u{200b}mple.com/"), Refusal::Invisible);
    assert_eq!(refused("https://example.com/\u{202e}x"), Refusal::Invisible);
    assert_eq!(refused("https://example.com/\u{2066}x"), Refusal::Invisible);
}

#[test]
fn ports_and_hosts_are_checked() {
    assert!(
        confirm("http://example.com:8080/x", "x")
            .shown
            .contains(":8080")
    );
    assert!(
        confirm("http://[::1]:8080/x", "x")
            .shown
            .contains("[::1]:8080")
    );
    assert_eq!(refused("http://example.com:/"), Refusal::Port);
    assert_eq!(refused("http://example.com:99999/"), Refusal::Port);
    assert_eq!(refused("http://example.com:80a/"), Refusal::Port);
    assert_eq!(refused("http://example.com:-1/"), Refusal::Port);
    for target in [
        "http:///x",
        "http://[zz]/",
        "http://-a.example/",
        "http://a-.example/",
        "http://a..example/",
        "http://a_b.example/",
        "http://exa mple.com/",
        "https:example.com",
    ] {
        assert!(
            matches!(check(target, "x"), Verdict::Refused(_)),
            "{target}"
        );
    }
}

#[test]
fn a_visible_text_that_looks_like_a_different_url_raises_the_mismatch_flag() {
    let flag = |target: &str, visible: &str| confirm(target, visible).mismatch;
    assert!(flag("https://evil.example/", "example.com"));
    assert!(flag("https://paypa1.example/", "www.paypal.example"));
    assert!(
        flag("https://example.com/", "http://example.com/"),
        "a downgrade is a difference"
    );
    assert!(flag("https://example.com/a", "https://example.com/b"));
    assert!(flag("https://xn--80ak6aa92e.com/", "apple.com"));
    assert!(!flag("https://example.com/", "https://example.com"));
    assert!(!flag("https://example.com/login", "example.com/login"));
    assert!(!flag("https://example.com/", "click here"));
    assert!(!flag("https://example.com/", "Download the report (PDF)"));
    assert!(!flag("https://example.com/", ""));
    assert!(!flag("mailto:a@lumenprotocol.org", "a@lumenprotocol.org"));
}

#[test]
fn mailto_keeps_only_the_address_cc_and_subject_and_shows_what_it_re_encodes() {
    let shown = |t: &str| confirm(t, "x").shown;
    assert_eq!(
        shown("mailto:a@lumenprotocol.org"),
        "mailto:a@lumenprotocol.org"
    );
    assert_eq!(
        shown(
            "mailto:a@lumenprotocol.org?subject=Hello%20World&body=secret&bcc=b@lumenprotocol.org&attach=/etc/passwd"
        ),
        "mailto:a@lumenprotocol.org?subject=Hello%20World"
    );
    assert_eq!(
        shown("mailto:a@lumenprotocol.org?cc=c@lumenprotocol.org&subject=hi"),
        "mailto:a@lumenprotocol.org?cc=c@lumenprotocol.org&subject=hi"
    );
    assert_eq!(
        shown("mailto:a@lumenprotocol.org?to=b@lumenprotocol.org"),
        "mailto:a@lumenprotocol.org,b@lumenprotocol.org"
    );
    assert_eq!(
        shown("mailto:a@lumenprotocol.org?SuBjEct=%C3%A9"),
        "mailto:a@lumenprotocol.org?subject=%C3%A9"
    );
    assert_eq!(
        shown("mailto:%61@lumenprotocol.org"),
        "mailto:a@lumenprotocol.org",
        "the address is decoded and then re-encoded"
    );
    assert_eq!(
        shown("mailto:a@lumenprotocol.org?body=line1%0Aline2"),
        "mailto:a@lumenprotocol.org",
        "a dropped value is not looked at"
    );
}

#[test]
fn mailto_is_split_on_the_literal_separators_before_decoding() {
    // %3F and %26 are not separators: they are part of the address, which refuses.
    assert_eq!(refused("mailto:a%3Fb@lumenprotocol.org"), Refusal::Address);
    assert_eq!(refused("mailto:a%26b@lumenprotocol.org"), Refusal::Address);
    // A literal `?` ends the address, and what is left has no `@`.
    assert_eq!(refused("mailto:a?b@lumenprotocol.org"), Refusal::Address);
    // A subject with an encoded `&` stays one value.
    assert_eq!(
        confirm("mailto:a@lumenprotocol.org?subject=a%26b", "x").shown,
        "mailto:a@lumenprotocol.org?subject=a%26b"
    );
}

#[test]
fn mailto_refuses_control_characters_in_what_it_keeps() {
    assert_eq!(refused("mailto:a%0A@lumenprotocol.org"), Refusal::Address);
    assert_eq!(refused("mailto:a%0D@lumenprotocol.org"), Refusal::Address);
    assert_eq!(
        refused("mailto:a@lumenprotocol.org?subject=hi%0D%0ABcc:x@lumenprotocol.org"),
        Refusal::MailValue
    );
    assert_eq!(
        refused("mailto:a@lumenprotocol.org?subject=hi%0Athere"),
        Refusal::MailValue
    );
    assert_eq!(
        refused("mailto:a@lumenprotocol.org?cc=b%0A@lumenprotocol.org"),
        Refusal::Address
    );
    assert_eq!(
        refused("mailto:a@lumenprotocol.org?subject=%00"),
        Refusal::MailValue
    );
    assert_eq!(refused("mailto:"), Refusal::Address);
    assert_eq!(refused("mailto:?subject=x"), Refusal::Address);
    assert_eq!(refused("mailto:not-an-address"), Refusal::Address);
    assert_eq!(refused("mailto:a@@lumenprotocol.org"), Refusal::Address);
}

#[test]
fn a_plain_copy_of_a_link_gives_its_visible_text_and_the_target_needs_the_confirmation() {
    let c = confirm("https://evil.example/", "https://bank.example/");
    assert_eq!(copy_plain("https://bank.example/"), "https://bank.example/");
    assert_eq!(copy_target(&c, false), None, "no confirmation, no target");
    assert_eq!(copy_target(&c, true), Some("https://evil.example/"));
}
