//! Small integer shape subgraphs stay on the host, retaining i64 Slice sentinels.
use super::*;

pub(super) fn fold(
    node: &NodeProto,
    values: &HashMap<String, Value>,
    data: &mut HashMap<String, Vec<i64>>,
    shapes: &mut HashMap<String, Vec<usize>>,
) -> Option<bool> {
    if node.output.len() != 1 {
        return Some(false);
    }
    let name = node.output[0].clone();
    if node.op_type == "Shape" {
        let shape = &values.get(node.input.first()?)?.shape;
        let rank = shape.len() as i64;
        let normalize = |v: i64| {
            if v < 0 {
                (v + rank).max(0)
            } else {
                v.min(rank)
            }
        };
        let start = normalize(integer(node, "start", 0));
        let end = normalize(integer(node, "end", rank)).max(start);
        let result = shape[start as usize..end as usize]
            .iter()
            .map(|&n| n as i64)
            .collect::<Vec<_>>();
        shapes.insert(name.clone(), vec![result.len()]);
        data.insert(name, result);
        return Some(true);
    }
    let Some(input) = node.input.first().and_then(|n| data.get(n)) else {
        return Some(false);
    };
    let shape = shapes.get(&node.input[0])?.clone();
    let (result, output) = match node.op_type.as_str() {
        "Add" | "Sub" | "Mul" | "Div" => {
            let rhs = data.get(node.input.get(1)?)?;
            if input.is_empty()
                || rhs.is_empty()
                || input.len() != rhs.len() && input.len() != 1 && rhs.len() != 1
            {
                return None;
            }
            let output = if input.len() == 1 {
                shapes.get(&node.input[1])?.clone()
            } else {
                shape
            };
            let mut result = Vec::new();
            for i in 0..input.len().max(rhs.len()) {
                let a = input[i % input.len()];
                let b = rhs[i % rhs.len()];
                result.push(match node.op_type.as_str() {
                    "Add" => a.checked_add(b)?,
                    "Sub" => a.checked_sub(b)?,
                    "Mul" => a.checked_mul(b)?,
                    _ => a.checked_div(b)?,
                });
            }
            (result, output)
        }
        "Gather" => {
            let indices = data.get(node.input.get(1)?)?;
            let axis = ops::axis(integer(node, "axis", 0), shape.len())?;
            let inner = shape[axis + 1..].iter().product::<usize>();
            let dim = shape[axis];
            let mut output = shape.clone();
            output.splice(axis..=axis, shapes.get(&node.input[1])?.iter().copied());
            let count = output.iter().product::<usize>();
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let index = indices[i / inner % indices.len()];
                let selected = if index < 0 { index + dim as i64 } else { index };
                if selected < 0 || selected >= dim as i64 {
                    return None;
                }
                result.push(
                    input[(i / (inner * indices.len()) * dim + selected as usize) * inner
                        + i % inner],
                );
            }
            (result, output)
        }
        "Unsqueeze" => {
            let axes = if let Some(name) = node.input.get(1) {
                data.get(name)?.clone()
            } else {
                integers(node, "axes", &[])
            };
            let mut chosen = vec![false; shape.len() + axes.len()];
            for a in axes {
                let i = ops::axis(a, chosen.len())?;
                if chosen[i] {
                    return None;
                }
                chosen[i] = true;
            }
            let mut it = shape.iter();
            let output = chosen
                .iter()
                .map(|&v| if v { Some(1) } else { it.next().copied() })
                .collect::<Option<Vec<_>>>()?;
            (input.clone(), output)
        }
        "Concat" => {
            let axis = ops::axis(integer(node, "axis", 0), shape.len())?;
            let inner = shape[axis + 1..].iter().product::<usize>();
            let outer = shape[..axis].iter().product::<usize>();
            let mut output = shape.clone();
            output[axis] = 0;
            for name in &node.input {
                let other = shapes.get(name)?;
                if other.len() != shape.len()
                    || other
                        .iter()
                        .zip(&shape)
                        .enumerate()
                        .any(|(i, (a, b))| i != axis && a != b)
                {
                    return None;
                }
                output[axis] = output[axis].checked_add(other[axis])?;
            }
            let mut result = Vec::new();
            for i in 0..outer {
                for name in &node.input {
                    let len = shapes.get(name)?[axis] * inner;
                    result.extend(&data.get(name)?[i * len..(i + 1) * len]);
                }
            }
            (result, output)
        }
        "Slice" => {
            let mut output = shape;
            let params = ops::slice(node, &mut output, data)?;
            let count = output.iter().product();
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let mut rem = i;
                let mut offset = 0i64;
                for d in (0..output.len()).rev() {
                    let p = 2 + d * 4;
                    offset += ((rem % output[d]) as i64 * params[p + 3] as i64
                        + params[p + 2] as i64)
                        * params[p + 1] as i64;
                    rem /= output[d];
                }
                result.push(*input.get(usize::try_from(offset).ok()?)?);
            }
            (result, output)
        }
        "Identity" => (input.clone(), shape),
        "Cast" if matches!(integer(node, "to", 0), 6 | 7) => {
            if integer(node, "to", 0) == 6 && input.iter().any(|&n| i32::try_from(n).is_err()) {
                return None;
            }
            (input.clone(), shape)
        }
        _ => return Some(false),
    };
    if result.len() > 4096 {
        return None;
    }
    data.insert(name.clone(), result);
    shapes.insert(name, output);
    Some(true)
}
