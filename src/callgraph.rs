use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_sarif::sarif::{
    CodeFlow, Location, LogicalLocation, Message, MultiformatMessageString, ReportingDescriptor,
    Result as SarifResult, ThreadFlow, ThreadFlowLocation,
};

use crate::classpath::ClasspathIndex;
use crate::ir::{CallKind, CallSite, Class, Method};

/// SARIF payload for call graph emission.
pub(crate) struct CallGraphResults {
    pub(crate) rules: Vec<ReportingDescriptor>,
    pub(crate) results: Vec<SarifResult>,
}

/// Build call graph results using a CHA baseline.
pub(crate) fn call_graph_results(
    classes: &[Class],
    _classpath: &ClasspathIndex,
) -> Result<CallGraphResults> {
    let hierarchy = build_hierarchy(classes);
    let methods = index_methods(classes);
    let edges = build_edges(classes, &hierarchy, &methods);

    if edges.is_empty() {
        return Ok(CallGraphResults {
            rules: Vec::new(),
            results: Vec::new(),
        });
    }

    let rule = ReportingDescriptor::builder()
        .id("CHA_CALL_GRAPH")
        .name("Call graph (CHA)")
        .short_description(
            MultiformatMessageString::builder()
                .text("CHA call graph path")
                .build(),
        )
        .build();

    let mut results = Vec::new();
    for edge in edges {
        let caller = format_method_id(&edge.caller);
        let callee = format_method_id(&edge.callee);
        let message = Message::builder()
            .text(format!("Call path: {caller} -> {callee}"))
            .build();

        let caller_location = ThreadFlowLocation::builder()
            .location(Location::builder().logical_locations(vec![method_location(&edge.caller)]).build())
            .kinds(vec!["call".to_string()])
            .build();
        let callee_location = ThreadFlowLocation::builder()
            .location(Location::builder().logical_locations(vec![method_location(&edge.callee)]).build())
            .kinds(vec!["call".to_string()])
            .build();
        let thread_flow = ThreadFlow::builder()
            .locations(vec![caller_location, callee_location])
            .build();
        let code_flow = CodeFlow::builder().thread_flows(vec![thread_flow]).build();

        let result = SarifResult::builder()
            .message(message)
            .rule_id("CHA_CALL_GRAPH")
            .code_flows(vec![code_flow])
            .build();
        results.push(result);
    }

    Ok(CallGraphResults {
        rules: vec![rule],
        results,
    })
}

/// Unique identifier for a method in the classpath.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct MethodId {
    class_name: String,
    name: String,
    descriptor: String,
}

/// Directed call edge between caller and callee.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CallEdge {
    caller: MethodId,
    callee: MethodId,
    kind: CallKind,
    offset: u32,
}

fn build_edges(
    classes: &[Class],
    hierarchy: &BTreeMap<String, Vec<String>>,
    methods: &BTreeMap<MethodId, ()>,
) -> BTreeSet<CallEdge> {
    let mut edges = BTreeSet::new();
    for class in classes {
        for method in &class.methods {
            let caller = MethodId {
                class_name: class.name.clone(),
                name: method.name.clone(),
                descriptor: method.descriptor.clone(),
            };
            for call in &method.calls {
                let targets = resolve_targets(call, hierarchy, methods);
                for callee in targets {
                    edges.insert(CallEdge {
                        caller: caller.clone(),
                        callee,
                        kind: call.kind,
                        offset: call.offset,
                    });
                }
            }
        }
    }
    edges
}

fn resolve_targets(
    call: &CallSite,
    hierarchy: &BTreeMap<String, Vec<String>>,
    methods: &BTreeMap<MethodId, ()>,
) -> Vec<MethodId> {
    let mut targets = Vec::new();
    let base = MethodId {
        class_name: call.owner.clone(),
        name: call.name.clone(),
        descriptor: call.descriptor.clone(),
    };
    match call.kind {
        CallKind::Static | CallKind::Special => {
            if methods.contains_key(&base) {
                targets.push(base);
            }
        }
        CallKind::Virtual | CallKind::Interface => {
            let mut candidates = vec![call.owner.clone()];
            if let Some(descendants) = hierarchy.get(&call.owner) {
                candidates.extend(descendants.iter().cloned());
            }
            candidates.sort();
            candidates.dedup();
            for class_name in candidates {
                let candidate = MethodId {
                    class_name,
                    name: call.name.clone(),
                    descriptor: call.descriptor.clone(),
                };
                if methods.contains_key(&candidate) {
                    targets.push(candidate);
                }
            }
        }
    }
    targets
}

fn build_hierarchy(classes: &[Class]) -> BTreeMap<String, Vec<String>> {
    let mut hierarchy = BTreeMap::new();
    for class in classes {
        if let Some(super_name) = &class.super_name {
            hierarchy
                .entry(super_name.clone())
                .or_insert_with(Vec::new)
                .push(class.name.clone());
        }
    }
    for descendants in hierarchy.values_mut() {
        descendants.sort();
        descendants.dedup();
    }
    hierarchy
}

fn index_methods(classes: &[Class]) -> BTreeMap<MethodId, ()> {
    let mut map = BTreeMap::new();
    for class in classes {
        for method in &class.methods {
            map.insert(
                MethodId {
                    class_name: class.name.clone(),
                    name: method.name.clone(),
                    descriptor: method.descriptor.clone(),
                },
                (),
            );
        }
    }
    map
}

fn method_location(method: &MethodId) -> LogicalLocation {
    LogicalLocation::builder()
        .name(format_method_id(method))
        .kind("function")
        .build()
}

fn format_method_id(method: &MethodId) -> String {
    format!(
        "{}.{}{}",
        method.class_name, method.name, method.descriptor
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_with_method(name: &str, super_name: Option<&str>, method: &Method) -> Class {
        Class {
            name: name.to_string(),
            super_name: super_name.map(str::to_string),
            referenced_classes: Vec::new(),
            methods: vec![method.clone()],
            artifact_index: 0,
        }
    }

    #[test]
    fn call_graph_includes_virtual_targets() {
        let caller = Method {
            name: "caller".to_string(),
            descriptor: "()V".to_string(),
            blocks: Vec::new(),
            calls: vec![CallSite {
                owner: "com/example/Base".to_string(),
                name: "target".to_string(),
                descriptor: "()V".to_string(),
                kind: CallKind::Virtual,
                offset: 0,
            }],
        };
        let base_method = Method {
            name: "target".to_string(),
            descriptor: "()V".to_string(),
            blocks: Vec::new(),
            calls: Vec::new(),
        };
        let subclass_method = base_method.clone();
        let classes = vec![
            class_with_method("com/example/Caller", None, &caller),
            class_with_method("com/example/Base", None, &base_method),
            class_with_method("com/example/Sub", Some("com/example/Base"), &subclass_method),
        ];
        let classpath = ClasspathIndex {
            classes: BTreeMap::new(),
        };

        let results = call_graph_results(&classes, &classpath).expect("call graph");

        assert!(!results.results.is_empty());
    }
}
