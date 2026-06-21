//! The shared 5etools inline-markup parser.
//!
//! 5etools entry strings carry inline tags `{@tag display|arg|arg}`. This module
//! lowers them two ways — [`strip_tags`] (plain text) and [`render_inline`]
//! (text + clickable cross-link [`Span`]s) — plus [`slugify`] for deriving the
//! `<slug>` primary key. It is the single best-factored seam of the reference-library
//! features: **both** the spell importer (`spell_import`) and the monster importer
//! (`monster_import`) parse markup through here, so neither owns it and the two can't
//! drift. Pure parsing — no spell/monster-specific knowledge leaks in.

use runebound_models::monsters::Span;

/// Whether a span run has any visible (non-whitespace) text. Shared by both
/// importers to drop empty entries.
pub(crate) fn spans_non_empty(spans: &[Span]) -> bool {
    spans.iter().any(|span| !span.text().trim().is_empty())
}

/// Strip 5etools inline tags `{@tag display|arg|arg}` to their visible text.
///
/// Rules (matching how 5etools renders): the display text is the LAST `|`-segment
/// when an explicit display is given (3+ segments, e.g.
/// `{@variantrule Sphere [Area of Effect]|XPHB|Sphere}` → "Sphere",
/// `{@scaledamage 8d6|3-9|1d6}` → "1d6"), otherwise the FIRST segment
/// (`{@damage 8d6}` → "8d6", `{@condition Prone|XPHB}` → "Prone"). `{@dc 15}` is
/// special-cased to "DC 15".
pub fn strip_tags(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && i + 1 < chars.len()
            && chars[i + 1] == '@'
            && let Some(close) = matching_brace(&chars, i)
        {
            let inner: String = chars[i + 2..close].iter().collect();
            out.push_str(&render_tag(&inner));
            i = close + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Index of the `}` that closes the `{` at `start`, tracking nested `{…}` so a
/// wrapper tag like `{@note See the {@cult …} entry.}` matches as one tag (not
/// closed early at the inner `}`). `None` if the braces never balance.
fn matching_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in chars[start..].iter().enumerate() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Rich-text wrapper tags carry recursively-rendered content (no `name|source`
/// args) — `{@i …}`, `{@b …}`, `{@note …}`, … — so their body is stripped/linked
/// recursively rather than split on `|`.
fn is_wrapper_tag(tag: &str) -> bool {
    matches!(
        tag,
        "i" | "italic"
            | "b"
            | "bold"
            | "u"
            | "underline"
            | "s"
            | "strike"
            | "note"
            | "highlight"
            | "comic"
            | "code"
            | "kbd"
            | "sub"
            | "sup"
    )
}

/// Like [`strip_tags`], but preserves cross-links: `{@spell …}` / `{@creature …}`
/// lower to clickable [`Span::Link`]s (targeting `spell <name>` / `monster <name>`),
/// while every other tag and all literal text collapse into [`Span::Text`] runs.
/// Adjacent text is merged, so a tag-free string yields a single `Text` span.
///
/// This shares [`render_tag`] with [`strip_tags`], so a link's visible label is
/// exactly the text `strip_tags` would have shown — only the click target is new.
pub fn render_inline(input: &str) -> Vec<Span> {
    let chars: Vec<char> = input.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && i + 1 < chars.len()
            && chars[i + 1] == '@'
            && let Some(close) = matching_brace(&chars, i)
        {
            let inner: String = chars[i + 2..close].iter().collect();
            let (tag, rest) = inner.split_once(' ').unwrap_or((inner.as_str(), ""));
            match tag_link(&inner) {
                Some((label, command)) => {
                    flush(&mut spans, &mut buf);
                    spans.push(Span::Link { label, command });
                }
                // A wrapper tag (`{@i …}`, `{@note … {@spell …} …}`) recurses so
                // any nested cross-links inside it survive as links.
                None if is_wrapper_tag(&tag.to_ascii_lowercase()) => {
                    flush(&mut spans, &mut buf);
                    spans.extend(render_inline(rest));
                }
                // Any other tag → its display text joins the running text run.
                None => buf.push_str(&render_tag(&inner)),
            }
            i = close + 1;
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut spans, &mut buf);
    coalesce_spans(spans)
}

/// Flush the pending text buffer into `spans` as one `Text` span (no-op if empty).
fn flush(spans: &mut Vec<Span>, buf: &mut String) {
    if !buf.is_empty() {
        spans.push(Span::Text {
            text: std::mem::take(buf),
        });
    }
}

/// Merge consecutive `Text` spans (a wrapper-tag recursion can leave adjacent runs).
pub(crate) fn coalesce_spans(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match (out.last_mut(), span) {
            (Some(Span::Text { text: prev }), Span::Text { text }) => prev.push_str(&text),
            (_, span) => out.push(span),
        }
    }
    out
}

/// If `inner` is a cross-linkable tag, return its `(visible label, command)`. The
/// label is the tag's normal display text (what [`strip_tags`] shows); the command
/// targets the canonical name — the FIRST `|`-segment — so
/// `{@creature Goblin|XMM|goblins}` links the displayed word "goblins" to
/// `monster Goblin`. Only `spell` and `creature` map to commands we have
/// (`spell`/`monster`); every other tag returns `None` and stays plain text.
fn tag_link(inner: &str) -> Option<(String, String)> {
    let (tag, rest) = inner.split_once(' ').unwrap_or((inner, ""));
    let target = rest.split('|').next().unwrap_or("").trim();
    if target.is_empty() {
        return None;
    }
    let command = match tag.to_ascii_lowercase().as_str() {
        "spell" => format!("spell {target}"),
        "creature" => format!("monster {target}"),
        _ => return None,
    };
    Some((render_tag(inner), command))
}

fn render_tag(inner: &str) -> String {
    let (tag, rest) = inner.split_once(' ').unwrap_or((inner, ""));
    let tag_lower = tag.to_ascii_lowercase();

    // Rich-text wrapper tags carry recursively-rendered content (no `|`-args):
    // `{@note See the {@cult …} entry.}` → "See the … entry.".
    if is_wrapper_tag(&tag_lower) {
        return strip_tags(rest);
    }

    // Monster stat-block tags that render a fixed phrase regardless of arguments,
    // including the no-argument ones (handled *before* the empty-`rest` guard
    // below). These tag names never appear in spell text, so extending the single
    // shared `strip_tags` seam is safe — the spell cases stay green.
    match tag_lower.as_str() {
        "h" => return "Hit: ".to_string(),
        "actsavefail" => return "Failure:".to_string(),
        "actsavesuccess" => return "Success:".to_string(),
        "actsavesuccessorfail" => return "Failure or Success:".to_string(),
        "acttrigger" => return "Trigger:".to_string(),
        "actresponse" => return "Response:".to_string(),
        "hityourspellattack" => return "your spell attack modifier".to_string(),
        "recharge" => return render_recharge(rest),
        "atk" => return render_attack(rest, AttackStyle::Weapon2014),
        "atkr" => return render_attack(rest, AttackStyle::Roll2024),
        _ => {}
    }

    if rest.is_empty() {
        return String::new();
    }
    let segments: Vec<&str> = rest.split('|').collect();
    match tag_lower.as_str() {
        "dc" => return format!("DC {}", segments[0].trim()),
        // Attack bonus: prepend a `+` unless the value already carries a sign.
        "hit" => return render_hit(segments[0].trim()),
        // `{@actSave dex}` → "Dexterity Saving Throw:".
        "actsave" => return format!("{} Saving Throw:", full_ability(segments[0].trim())),
        // `{@filter Mountain|bestiary|environment=mountain}` → "Mountain" (the
        // visible label is the FIRST segment, not the last as the generic rule
        // assumes — the trailing segments are filter query params). Common in fluff.
        "filter" => return segments[0].trim().to_string(),
        _ => {}
    }
    let display = if segments.len() >= 3 {
        segments[segments.len() - 1]
    } else {
        segments[0]
    };
    // Recurse: a display segment can itself hold a nested tag (rare, but real).
    strip_tags(display.trim())
}

/// `{@hit 4}` → "+4"; a value that already carries a sign is kept verbatim.
fn render_hit(value: &str) -> String {
    if value.starts_with('+') || value.starts_with('-') {
        value.to_string()
    } else {
        format!("+{value}")
    }
}

/// `{@recharge}` / `{@recharge 6}` → "(Recharge 6)"; `{@recharge 5}` →
/// "(Recharge 5-6)" (recharges on a d6 roll of N or higher).
fn render_recharge(rest: &str) -> String {
    let n: u32 = rest.trim().parse().unwrap_or(6);
    if n >= 6 {
        "(Recharge 6)".to_string()
    } else {
        format!("(Recharge {n}-6)")
    }
}

/// Whether an attack tag is the 2014 weapon/spell form (`{@atk mw}`) or the 2024
/// attack-roll form (`{@atkr m}`).
enum AttackStyle {
    Weapon2014,
    Roll2024,
}

/// Render `{@atk mw,rw}` → "Melee or Ranged Weapon Attack:" and
/// `{@atkr m,r}` → "Melee or Ranged Attack Roll:". Each comma-separated part is a
/// range char (`m`/`r`) optionally followed by a kind char (`w`/`s`).
fn render_attack(spec: &str, style: AttackStyle) -> String {
    let mut ranges: Vec<&str> = Vec::new();
    let mut kind = "";
    for part in spec.split(',') {
        let mut chars = part.trim().chars();
        match chars.next() {
            Some('m' | 'M') => push_unique(&mut ranges, "Melee"),
            Some('r' | 'R') => push_unique(&mut ranges, "Ranged"),
            _ => {}
        }
        if let Some(second) = chars.next() {
            kind = match second {
                'w' | 'W' => "Weapon ",
                's' | 'S' => "Spell ",
                _ => kind,
            };
        }
    }
    let ranges = if ranges.is_empty() {
        "Melee".to_string()
    } else {
        ranges.join(" or ")
    };
    match style {
        AttackStyle::Roll2024 => format!("{ranges} Attack Roll:"),
        AttackStyle::Weapon2014 => format!("{ranges} {kind}Attack:"),
    }
}

fn push_unique<'a>(values: &mut Vec<&'a str>, item: &'a str) {
    if !values.contains(&item) {
        values.push(item);
    }
}

/// Expand a 3-letter ability abbreviation to its full name (for `{@actSave dex}`).
fn full_ability(abbr: &str) -> &'static str {
    match abbr.to_ascii_lowercase().as_str() {
        "str" => "Strength",
        "dex" => "Dexterity",
        "con" => "Constitution",
        "int" => "Intelligence",
        "wis" => "Wisdom",
        "cha" => "Charisma",
        _ => "Special",
    }
}

/// Kebab-case a spell/monster name into its slug — the primary key shared by the
/// TOML store and the search DB. Public so lookups can derive a slug from a typed
/// name.
///
/// This is **not** the same as [`runebound_models::utils::slugify`]: that one keeps
/// only whitespace/`-`/`_`/`.` as separators (dropping other punctuation) and falls
/// back to `"untitled"`, whereas this turns *every* non-alphanumeric run into a
/// single dash. They yield different slugs for punctuated names (e.g. "Tasha's" →
/// `tasha-s` here vs `tashas` there), so they must stay separate — changing this one
/// would orphan every stored `<slug>.toml` reference card.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_tags_handles_every_tag_shape() {
        assert_eq!(
            strip_tags("deals {@damage 8d6} Fire damage"),
            "deals 8d6 Fire damage"
        );
        assert_eq!(
            strip_tags("the {@condition Prone|XPHB} condition"),
            "the Prone condition"
        );
        assert_eq!(strip_tags("a {@dc 15} save"), "a DC 15 save");
        assert_eq!(
            strip_tags("a 20-foot {@variantrule Sphere [Area of Effect]|XPHB|Sphere}"),
            "a 20-foot Sphere"
        );
        assert_eq!(
            strip_tags("increases by {@scaledamage 8d6|3-9|1d6} for each slot"),
            "increases by 1d6 for each slot"
        );
        assert_eq!(strip_tags("no tags here"), "no tags here");
    }

    #[test]
    fn strip_tags_handles_monster_stat_block_tags() {
        // Attack lines (2024 + 2014 forms).
        assert_eq!(strip_tags("{@atkr m}"), "Melee Attack Roll:");
        assert_eq!(strip_tags("{@atkr m,r}"), "Melee or Ranged Attack Roll:");
        assert_eq!(strip_tags("{@atk mw}"), "Melee Weapon Attack:");
        assert_eq!(strip_tags("{@atk mw,rw}"), "Melee or Ranged Weapon Attack:");
        // Hit bonus + the "Hit:" lead-in.
        assert_eq!(strip_tags("{@hit 4}"), "+4");
        assert_eq!(strip_tags("{@hit -1}"), "-1");
        assert_eq!(strip_tags("{@h}5 damage"), "Hit: 5 damage");
        // Saving throws + outcomes.
        assert_eq!(
            strip_tags("{@actSave dex} {@dc 21}"),
            "Dexterity Saving Throw: DC 21"
        );
        assert_eq!(strip_tags("{@actSaveFail} 59 damage"), "Failure: 59 damage");
        assert_eq!(strip_tags("{@actSaveSuccess} Half"), "Success: Half");
        // Recharge (in an action name).
        assert_eq!(
            strip_tags("Fire Breath {@recharge 5}"),
            "Fire Breath (Recharge 5-6)"
        );
        assert_eq!(strip_tags("Breath {@recharge}"), "Breath (Recharge 6)");
        // A full goblin attack line lowers cleanly.
        assert_eq!(
            strip_tags(
                "{@atkr m} {@hit 4}, reach 5 ft. {@h}5 ({@damage 1d6 + 2}) Slashing damage."
            ),
            "Melee Attack Roll: +4, reach 5 ft. Hit: 5 (1d6 + 2) Slashing damage."
        );
        // `{@filter}` (common in fluff) shows its first segment, not the query params.
        assert_eq!(
            strip_tags("found in {@filter Mountains|bestiary|environment=mountain}"),
            "found in Mountains"
        );
        // Nested wrapper tags: the outer `{@note …}` must match its OWN closing
        // brace (not the inner tag's), and the inner reference still resolves.
        assert_eq!(
            strip_tags("{@note See the {@cult Cult of Baphomet|MPMM} entry.}"),
            "See the Cult of Baphomet entry."
        );
        assert_eq!(
            strip_tags("an {@i italic {@b bold}} word"),
            "an italic bold word"
        );
    }

    #[test]
    fn render_inline_keeps_spell_and_creature_links() {
        // `{@spell}` → a Link to the spellbook; surrounding text stays plain and merges.
        assert_eq!(
            render_inline("The lich casts {@spell Fireball|XPHB} at will."),
            vec![
                Span::Text {
                    text: "The lich casts ".to_string()
                },
                Span::Link {
                    label: "Fireball".to_string(),
                    command: "spell Fireball".to_string()
                },
                Span::Text {
                    text: " at will.".to_string()
                },
            ]
        );
        // `{@creature}` → a Link to the bestiary; a 3-segment tag shows the display
        // word but targets the canonical creature name (the first segment).
        assert_eq!(
            render_inline("summons {@creature Goblin Boss|XMM|goblin bosses}"),
            vec![
                Span::Text {
                    text: "summons ".to_string()
                },
                Span::Link {
                    label: "goblin bosses".to_string(),
                    command: "monster Goblin Boss".to_string()
                },
            ]
        );
        // Non-link tags collapse into the text run; a tag-free string is one span.
        assert_eq!(
            render_inline("deals {@damage 8d6} fire damage"),
            vec![Span::Text {
                text: "deals 8d6 fire damage".to_string()
            }]
        );
        assert_eq!(
            render_inline("plain prose"),
            vec![Span::Text {
                text: "plain prose".to_string()
            }]
        );
        // A link nested inside a wrapper tag survives as a link; surrounding text
        // merges into single runs on each side.
        assert_eq!(
            render_inline("{@note The lich casts {@spell Fireball|XPHB} here.}"),
            vec![
                Span::Text {
                    text: "The lich casts ".to_string()
                },
                Span::Link {
                    label: "Fireball".to_string(),
                    command: "spell Fireball".to_string()
                },
                Span::Text {
                    text: " here.".to_string()
                },
            ]
        );
    }
}
