use super::*;
use lerp::Lerp;
use uuid::Uuid;

#[test]
fn lerp_float() {
    let a = AnimationParameterValue::Float(0.0);
    let b = AnimationParameterValue::Float(10.0);
    let AnimationParameterValue::Float(v) = a.lerp(b, 0.5) else {
        panic!()
    };
    assert!((v - 5.0).abs() < 1e-5);
}

#[test]
fn lerp_int() {
    let a = AnimationParameterValue::Int(0);
    let b = AnimationParameterValue::Int(10);
    let AnimationParameterValue::Int(v) = a.lerp(b, 0.8) else {
        panic!()
    };
    assert_eq!(v, 8);
}

#[test]
fn lerp_bool_threshold() {
    let f = AnimationParameterValue::Bool(false);
    let t = AnimationParameterValue::Bool(true);
    let AnimationParameterValue::Bool(v) = f.lerp(t, 0.4) else {
        panic!()
    };
    assert!(!v);
    let AnimationParameterValue::Bool(v) = f.lerp(t, 0.6) else {
        panic!()
    };
    assert!(v);
}

fn make_params(id: Uuid, val: f32) -> HashMap<Uuid, AnimationParameterValue> {
    let mut m = HashMap::new();
    m.insert(id, AnimationParameterValue::Float(val));
    m
}

#[test]
fn nearest_neighbor_exact_match_weight_one() {
    let pid = Uuid::new_v4();
    let tree: BlendTree<1> = BlendTree {
        parameters: vec![pid],
        motions: vec![
            BlendTreeMotion {
                threshold: [0.0],
                motion: AnimationMotion::default(),
            },
            BlendTreeMotion {
                threshold: [1.0],
                motion: AnimationMotion::default(),
            },
        ],
    };
    let params = make_params(pid, 0.0);
    let neighbors = tree.nearest_neighbors(2, &params);
    assert_eq!(neighbors.len(), 1);
    assert!((neighbors[0].0 - 1.0).abs() < 1e-5);
}

#[test]
fn nearest_neighbors_weights_sum_to_one() {
    let pid = Uuid::new_v4();
    let tree: BlendTree<1> = BlendTree {
        parameters: vec![pid],
        motions: vec![
            BlendTreeMotion {
                threshold: [0.0],
                motion: AnimationMotion::default(),
            },
            BlendTreeMotion {
                threshold: [1.0],
                motion: AnimationMotion::default(),
            },
        ],
    };
    let params = make_params(pid, 0.5);
    let neighbors = tree.nearest_neighbors(2, &params);
    let total: f32 = neighbors.iter().map(|(w, _)| w).sum();
    assert!((total - 1.0).abs() < 1e-5);
}

#[test]
fn nearest_neighbors_missing_param_returns_empty() {
    let tree: BlendTree<1> = BlendTree {
        parameters: vec![Uuid::new_v4()],
        motions: vec![BlendTreeMotion {
            threshold: [0.0],
            motion: AnimationMotion::default(),
        }],
    };
    let neighbors = tree.nearest_neighbors(2, &HashMap::new());
    assert!(neighbors.is_empty());
}

#[test]
fn nearest_neighbors_empty_tree_returns_empty() {
    let pid = Uuid::new_v4();
    let tree: BlendTree<1> = BlendTree {
        parameters: vec![pid],
        motions: vec![],
    };
    assert!(tree.nearest_neighbors(2, &make_params(pid, 0.5)).is_empty());
}
