//! Guild member list from `GUILD_MEMBER_LIST_UPDATE`.
//!
//! Discord sends this after an opcode 14 subscription. A SYNC replaces the
//! visible range. INSERT, UPDATE, and DELETE patch one slot. INVALIDATE drops
//! the list so the client subscribes again.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterMember {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub activity: String,
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterGroup {
    pub name: String,
    pub count: u32,
    pub members: Vec<RosterMember>,
}

pub fn apply_member_list(groups: &mut Vec<RosterGroup>, data: &Value) -> bool {
    let Some(ops) = data.get("ops").and_then(|ops| ops.as_array()) else {
        return false;
    };
    let mut changed = false;
    for op in ops {
        let kind = op.get("op").and_then(|op| op.as_str()).unwrap_or("");
        match kind {
            "SYNC" => {
                if let Some(items) = op.get("items").and_then(|items| items.as_array()) {
                    *groups = groups_from_items(items);
                    changed = true;
                }
            }
            "INVALIDATE" => {
                groups.clear();
                changed = true;
            }
            "DELETE" => {
                if let Some(index) = op.get("index").and_then(|index| index.as_u64()) {
                    remove_slot(groups, index as usize);
                    changed = true;
                }
            }
            "INSERT" | "UPDATE" => {
                let index = op
                    .get("index")
                    .and_then(|index| index.as_u64())
                    .unwrap_or(0) as usize;
                if let Some(item) = op.get("item") {
                    if kind == "UPDATE" {
                        remove_slot(groups, index);
                    }
                    insert_slot(groups, index, item);
                    changed = true;
                }
            }
            _ => {}
        }
    }
    changed
}

fn groups_from_items(items: &[Value]) -> Vec<RosterGroup> {
    let mut groups = Vec::new();
    for item in items {
        push_item(&mut groups, item);
    }
    groups
}

fn push_item(groups: &mut Vec<RosterGroup>, item: &Value) {
    if let Some(group) = item.get("group") {
        let name = group_name(
            group
                .get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("online"),
        );
        let count = group
            .get("count")
            .and_then(|count| count.as_u64())
            .unwrap_or(0) as u32;
        groups.push(RosterGroup {
            name,
            count,
            members: Vec::new(),
        });
        return;
    }
    if let Some(member) = member_from_item(item) {
        if groups.is_empty() {
            groups.push(RosterGroup {
                name: "Online".into(),
                count: 0,
                members: Vec::new(),
            });
        }
        if let Some(group) = groups.last_mut() {
            group.members.push(member);
        }
    }
}

fn insert_slot(groups: &mut Vec<RosterGroup>, index: usize, item: &Value) {
    if item.get("group").is_some() {
        push_item(groups, item);
        return;
    }
    let Some(member) = member_from_item(item) else {
        return;
    };
    let mut seen = 0usize;
    for group in groups.iter_mut() {
        if index <= seen + group.members.len() {
            let at = index.saturating_sub(seen).min(group.members.len());
            group.members.insert(at, member);
            return;
        }
        seen += group.members.len() + 1;
    }
    push_item(groups, item);
}

fn remove_slot(groups: &mut [RosterGroup], index: usize) {
    let mut seen = 0usize;
    for group in groups.iter_mut() {
        if index == seen {
            return;
        }
        let start = seen + 1;
        if index < start + group.members.len() {
            group.members.remove(index - start);
            return;
        }
        seen = start + group.members.len();
    }
}

fn member_from_item(item: &Value) -> Option<RosterMember> {
    let member = item.get("member")?;
    let user = member.get("user")?;
    let id = user
        .get("id")
        .and_then(|id| id.as_str())
        .and_then(|id| id.parse().ok())
        .unwrap_or(0);
    let name = user
        .get("global_name")
        .and_then(|name| name.as_str())
        .or_else(|| user.get("username").and_then(|name| name.as_str()))
        .unwrap_or("Member")
        .to_owned();
    let presence = member.get("presence");
    let status = presence
        .and_then(|presence| presence.get("status"))
        .and_then(|status| status.as_str())
        .unwrap_or("offline")
        .to_owned();
    let activity = activity_line(presence);
    let avatar = user
        .get("avatar")
        .and_then(|avatar| avatar.as_str())
        .map(str::to_owned);
    Some(RosterMember {
        id,
        name,
        status,
        activity,
        avatar,
    })
}

fn activity_line(presence: Option<&Value>) -> String {
    let Some(activity) = presence
        .and_then(|presence| presence.get("activities"))
        .and_then(|activities| activities.as_array())
        .and_then(|activities| activities.first())
    else {
        return String::new();
    };
    let name = activity
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or("");
    if name.eq_ignore_ascii_case("In voice") {
        return "In voice".into();
    }
    match activity
        .get("type")
        .and_then(|kind| kind.as_u64())
        .unwrap_or(0)
    {
        0 if !name.is_empty() => format!("Playing {name}"),
        2 if !name.is_empty() => format!("Listening to {name}"),
        4 => activity
            .get("state")
            .and_then(|state| state.as_str())
            .unwrap_or("Custom status")
            .to_owned(),
        _ if name.eq_ignore_ascii_case("In voice") || name.is_empty() => "In voice".into(),
        _ => name.to_owned(),
    }
}

fn group_name(id: &str) -> String {
    match id {
        "online" => "Online".into(),
        "idle" => "Idle".into(),
        "dnd" => "Do Not Disturb".into(),
        "offline" => "Offline".into(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sync_builds_role_groups_and_delete_removes_one_member() {
        let payload = serde_json::json!({
            "ops": [{
                "op": "SYNC",
                "range": [0, 99],
                "items": [
                    {"group": {"id": "online", "count": 1}},
                    {"member": {"user": {"id": "2", "global_name": "Quacky"}, "presence": {"status": "online", "activities": [{"type": 0, "name": "THE FINALS"}]}}},
                    {"group": {"id": "Ranked Players", "count": 2}},
                    {"member": {"user": {"id": "3", "username": "aryann"}, "presence": {"status": "online", "activities": [{"name": "In voice"}]}}},
                    {"member": {"user": {"id": "4", "global_name": "Boyka"}, "presence": {"status": "idle", "activities": [{"type": 0, "name": "ROBLOX"}]}}}
                ]
            }]
        });
        let mut groups = Vec::new();
        assert!(apply_member_list(&mut groups, &payload));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "Online");
        assert_eq!(groups[0].count, 1);
        assert_eq!(groups[0].members[0].activity, "Playing THE FINALS");
        assert_eq!(groups[1].name, "Ranked Players");
        assert_eq!(groups[1].count, 2);
        assert_eq!(groups[1].members[0].activity, "In voice");
        assert_eq!(groups[1].members[1].activity, "Playing ROBLOX");
        let remove = serde_json::json!({"ops": [{"op": "DELETE", "index": 3}]});
        assert!(apply_member_list(&mut groups, &remove));
        assert_eq!(groups[1].members.len(), 1);
        assert_eq!(groups[1].members[0].name, "Boyka");
    }
}
