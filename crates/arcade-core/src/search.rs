use crate::storage::RankingSignals;
use arcade_contract::ToolManifest;

/// The base relevance tier dominates usage and favorites. This keeps an exact
/// match reachable even after another tool becomes frequent.
pub fn rank<'a>(
    tools: &'a [ToolManifest],
    query: &str,
    signals: &RankingSignals,
) -> Vec<&'a ToolManifest> {
    let query = normalize(query);
    let mut scored: Vec<_> = tools
        .iter()
        .filter_map(|tool| {
            let relevance = relevance(tool, &query);
            if !query.is_empty() && relevance == 0 {
                return None;
            }
            let usage = signals.uses.get(&tool.id).copied().unwrap_or(0);
            let history_bonus = usage.min(20) as i64;
            let favorite_bonus = if signals.favorites.contains(&tool.id) {
                25
            } else {
                0
            };
            Some((tool, relevance * 1_000 + favorite_bonus + history_bonus))
        })
        .collect();
    scored.sort_by(|(a, a_score), (b, b_score)| {
        b_score.cmp(a_score).then_with(|| a.name.cmp(&b.name))
    });
    scored.into_iter().map(|(tool, _)| tool).collect()
}

fn relevance(tool: &ToolManifest, query: &str) -> i64 {
    if query.is_empty() {
        return 1;
    }
    let name = normalize(&tool.name);
    let aliases: Vec<_> = tool
        .aliases
        .iter()
        .chain(tool.phrases.iter())
        .map(|s| normalize(s))
        .collect();
    if name == query {
        return 100;
    }
    if aliases.iter().any(|s| s == query) {
        return 95;
    }
    if name.starts_with(query) {
        return 88;
    }
    if aliases.iter().any(|s| s.starts_with(query)) {
        return 84;
    }
    if name.contains(query) {
        return 78;
    }
    if aliases.iter().any(|s| s.contains(query)) {
        return 75;
    }

    let words: Vec<_> = query.split_whitespace().collect();
    let corpus = format!(
        "{} {} {} {}",
        name,
        aliases.join(" "),
        normalize(&tool.description),
        normalize(&tool.category)
    );
    let matched = words
        .iter()
        .filter(|word| {
            corpus
                .split_whitespace()
                .any(|candidate| candidate == **word || candidate.starts_with(**word))
        })
        .count();
    if matched == words.len() {
        return 65;
    }
    if words.len() > 1 && matched > 0 {
        return 20 + (matched as i64 * 8);
    }

    let mut best = strsim::jaro_winkler(&name, query);
    for alias in &aliases {
        best = best.max(strsim::jaro_winkler(alias, query));
    }
    if best >= 0.82 {
        return (best * 50.0) as i64;
    }
    0
}

fn normalize(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Arcade;

    #[test]
    fn exact_match_beats_favorite() {
        let runtime = Arcade::in_memory().unwrap();
        let signals = RankingSignals {
            favorites: ["arcade.pdf.merge".to_string()].into(),
            uses: [("arcade.pdf.merge".to_string(), 10_000)].into(),
        };
        let tools = runtime.list_tools();
        let result = rank(&tools, "image resize", &signals);
        assert_eq!(result[0].name, "Image Resize / Resample");
    }

    #[test]
    fn aliases_resolve_pdf_merge() {
        let runtime = Arcade::in_memory().unwrap();
        let tools = runtime.list_tools();
        let result = rank(&tools, "join pdf", &RankingSignals::default());
        assert_eq!(result[0].name, "Merge PDFs");
    }
}
