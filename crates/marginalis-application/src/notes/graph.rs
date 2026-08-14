//! ノートと文献情報のグラフビュー、および表示範囲の絞り込み。

use std::collections::{HashMap, HashSet};

use marginalis_domain::{MAX_GRAPH_DEPTH, NoteId, UnixMillis};

/// グラフビューに出す点と線。
///
/// 点は現在の利用者が閲覧できるノートと、そのノートが引用している文献だけとする。線は始点と
/// 終点の両方が点として出る場合だけ返す。閲覧できないノートの存在も件数も現れない。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoteGraph {
    pub notes: Vec<NoteGraphNote>,
    pub works: Vec<NoteGraphWork>,
    pub references: Vec<NoteGraphReference>,
    pub citations: Vec<NoteGraphCitation>,
}

/// 図に出すノート。本文は含めない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphNote {
    pub note_id: NoteId,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at: UnixMillis,
}

/// 図に出す文献。文献ライブラリの内容ではなく、引用されたという事実だけを表す。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphWork {
    pub citation_key: String,
    /// 引用元のノートを書いた利用者のライブラリで解決できた場合の題名。
    pub title: Option<String>,
}

/// ノートからノートへの参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphReference {
    pub source_note_id: NoteId,
    pub target_note_id: NoteId,
}

/// ノートから文献への引用。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphCitation {
    pub source_note_id: NoteId,
    pub citation_key: String,
}

/// 図に出す範囲の指定。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoteGraphQuery {
    /// 題名、本文、タグのいずれかにこの語を含むノートだけへ絞る。
    pub text: Option<String>,
    /// 起点のノート。指定すると、そこから[`NoteGraphQuery::depth`]階層以内だけを残す。
    pub origin: Option<NoteId>,
    /// 起点から数えて何本の線を辿るか。起点を指定しない場合は使わない。
    pub depth: Option<u32>,
}

impl NoteGraph {
    /// 起点から`depth`本以内の線で辿れる点と、その間の線だけを残す。
    ///
    /// 認可は問い合わせの側で済んでいる。ここで扱うのは、閲覧できる範囲のうちどこを見せるかと
    /// いう表示上の絞り込みだけである。起点が図に無い場合は空の図を返す。
    pub fn within(self, origin: NoteId, depth: u32) -> Self {
        let works = |key: &str| format!("work:{key}");
        let note_key = |note_id: NoteId| format!("note:{note_id}");
        if !self.notes.iter().any(|note| note.note_id == origin) {
            return Self::default();
        }

        // 参照と引用をどちらも双方向に辿る。向きを問わないのは、離れた話題のつながりを
        // 見つけることが図の目的だからである。
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        let mut join = |left: String, right: String| {
            adjacency
                .entry(left.clone())
                .or_default()
                .push(right.clone());
            adjacency.entry(right).or_default().push(left);
        };
        for edge in &self.references {
            join(note_key(edge.source_note_id), note_key(edge.target_note_id));
        }
        for edge in &self.citations {
            join(note_key(edge.source_note_id), works(&edge.citation_key));
        }

        let mut reached: HashSet<String> = HashSet::new();
        let mut frontier = vec![note_key(origin)];
        reached.insert(note_key(origin));
        for _ in 0..depth.min(MAX_GRAPH_DEPTH) {
            let mut next = Vec::new();
            for vertex in frontier {
                for neighbour in adjacency.get(&vertex).into_iter().flatten() {
                    if reached.insert(neighbour.clone()) {
                        next.push(neighbour.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        Self {
            notes: self
                .notes
                .into_iter()
                .filter(|note| reached.contains(&note_key(note.note_id)))
                .collect(),
            works: self
                .works
                .into_iter()
                .filter(|work| reached.contains(&works(&work.citation_key)))
                .collect(),
            references: self
                .references
                .into_iter()
                .filter(|edge| {
                    reached.contains(&note_key(edge.source_note_id))
                        && reached.contains(&note_key(edge.target_note_id))
                })
                .collect(),
            citations: self
                .citations
                .into_iter()
                .filter(|edge| {
                    reached.contains(&note_key(edge.source_note_id))
                        && reached.contains(&works(&edge.citation_key))
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::EntityId;

    use super::*;

    fn graph_note_id(last: u32) -> NoteId {
        NoteId::new(
            EntityId::from_str(&format!("0197c9bc-0000-7000-8000-{last:012x}")).expect("note ID"),
        )
    }

    /// 鎖状の4件のノートと、末尾のノートが引用する文献1件からなる図を作る。
    fn chain_graph() -> NoteGraph {
        NoteGraph {
            notes: (1..=4)
                .map(|index| NoteGraphNote {
                    note_id: graph_note_id(index),
                    title: format!("ノート{index}"),
                    tags: Vec::new(),
                    updated_at: UnixMillis::new(index.into()),
                })
                .collect(),
            works: vec![NoteGraphWork {
                citation_key: "smith2024".into(),
                title: None,
            }],
            references: (1..4)
                .map(|index| NoteGraphReference {
                    source_note_id: graph_note_id(index),
                    target_note_id: graph_note_id(index + 1),
                })
                .collect(),
            citations: vec![NoteGraphCitation {
                source_note_id: graph_note_id(4),
                citation_key: "smith2024".into(),
            }],
        }
    }

    #[test]
    fn keeps_only_what_is_reachable_from_the_origin() {
        let within_one = chain_graph().within(graph_note_id(1), 1);
        assert_eq!(
            within_one
                .notes
                .iter()
                .map(|note| note.title.as_str())
                .collect::<Vec<_>>(),
            ["ノート1", "ノート2"]
        );
        assert_eq!(within_one.references.len(), 1);
        assert!(within_one.works.is_empty());

        let within_three = chain_graph().within(graph_note_id(1), 3);
        assert_eq!(within_three.notes.len(), 4);
        assert!(within_three.works.is_empty());
        assert!(within_three.citations.is_empty());

        let within_four = chain_graph().within(graph_note_id(1), 4);
        assert_eq!(within_four.works.len(), 1);
        assert_eq!(within_four.citations.len(), 1);
        assert_eq!(chain_graph().within(graph_note_id(4), 3).notes.len(), 4);
        assert_eq!(
            chain_graph().within(graph_note_id(1), MAX_GRAPH_DEPTH + 10),
            chain_graph().within(graph_note_id(1), MAX_GRAPH_DEPTH)
        );
    }

    #[test]
    fn is_empty_when_the_origin_is_not_visible() {
        assert_eq!(
            chain_graph().within(graph_note_id(99), 3),
            NoteGraph::default()
        );
    }
}
