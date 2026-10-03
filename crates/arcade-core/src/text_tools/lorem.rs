//! Classic "Lorem ipsum" placeholder text.

use crate::tool_kit::{number_in, option_bool, option_str};
use arcade_contract::ToolRequest;
use rand::{Rng, seq::IndexedRandom};

const OPENING: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit";
const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "duis",
    "aute",
    "irure",
    "in",
    "reprehenderit",
    "voluptate",
    "velit",
    "esse",
    "cillum",
    "fugiat",
    "nulla",
    "pariatur",
    "excepteur",
    "sint",
    "occaecat",
    "cupidatat",
    "non",
    "proident",
    "sunt",
    "culpa",
    "qui",
    "officia",
    "deserunt",
    "mollit",
    "anim",
    "id",
    "est",
    "laborum",
    "praesent",
    "viverra",
    "integer",
    "feugiat",
    "pellentesque",
    "habitant",
    "morbi",
    "tristique",
    "senectus",
    "netus",
    "malesuada",
    "fames",
    "turpis",
    "egestas",
    "mauris",
    "vitae",
    "ultricies",
    "leo",
    "porta",
    "lacus",
    "vel",
];

pub(super) fn generate(request: &ToolRequest) -> Result<String, String> {
    let count = number_in(request, "count", "Amount", Some(3.0), 1.0..=500.0)? as usize;
    let classic = option_bool(request, "classic", true);
    let mut rng = rand::rng();
    let mut sentence = |first: bool| {
        let length = rng.random_range(8..=16);
        let mut words = (0..length)
            .map(|_| *WORDS.choose(&mut rng).expect("word list is not empty"))
            .collect::<Vec<_>>();
        if first && classic {
            words.splice(0..words.len().min(8), OPENING.split(' '));
        }
        let mut text = words.join(" ");
        if let Some(first_char) = text.get(..1) {
            text.replace_range(..1, &first_char.to_uppercase());
        }
        text.push('.');
        text
    };
    let output = match option_str(request, "unit", "paragraphs") {
        "paragraphs" => (0..count)
            .map(|index| {
                (0..5)
                    .map(|position| sentence(index == 0 && position == 0))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        "sentences" => (0..count)
            .map(|index| sentence(index == 0))
            .collect::<Vec<_>>()
            .join(" "),
        "words" => {
            let mut words = Vec::with_capacity(count);
            if classic {
                words.extend(
                    OPENING
                        .replace(',', "")
                        .split(' ')
                        .take(count)
                        .map(str::to_owned),
                );
            }
            while words.len() < count {
                words.push((*WORDS.choose(&mut rng).expect("word list is not empty")).to_owned());
            }
            words.join(" ")
        }
        other => return Err(format!("Unknown unit: {other}")),
    };
    Ok(output)
}
