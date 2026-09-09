use crate::config::WorkspaceMember;
use anyhow::{bail, Result};

pub fn ordered_members(members: &[WorkspaceMember]) -> Result<Vec<&WorkspaceMember>> {
    let publish: Vec<&WorkspaceMember> = members.iter().filter(|m| m.publish).collect();

    let mut ordered: Vec<&WorkspaceMember> = Vec::new();
    let mut remaining: Vec<&WorkspaceMember> = publish.clone();
    let mut iterations = 0;
    let max = remaining.len() * remaining.len() + 1;

    while !remaining.is_empty() {
        iterations += 1;
        if iterations > max {
            bail!("circular dependency detected in workspace members");
        }

        let mut progress = false;
        remaining.retain(|member| {
            let deps_satisfied = member
                .depends_on
                .iter()
                .all(|dep| ordered.iter().any(|m| &m.name == dep));

            if deps_satisfied {
                ordered.push(member);
                progress = true;
                false
            } else {
                true
            }
        });

        if !progress {
            bail!("circular dependency detected in workspace members");
        }
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_members_lib_before_cli() {
        let members = vec![
            WorkspaceMember {
                name: "mycli".into(),
                path: "cli".into(),
                publish: true,
                depends_on: vec!["mylib".into()],
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered[0].name, "mylib");
        assert_eq!(ordered[1].name, "mycli");
    }

    #[test]
    fn ordered_members_no_deps_preserves_order() {
        let members = vec![
            WorkspaceMember {
                name: "a".into(),
                path: "a".into(),
                publish: true,
                depends_on: vec![],
            },
            WorkspaceMember {
                name: "b".into(),
                path: "b".into(),
                publish: true,
                depends_on: vec![],
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered[0].name, "a");
        assert_eq!(ordered[1].name, "b");
    }

    #[test]
    fn ordered_members_skips_unpublished() {
        let members = vec![
            WorkspaceMember {
                name: "internal".into(),
                path: "internal".into(),
                publish: false,
                depends_on: vec![],
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].name, "mylib");
    }
}
