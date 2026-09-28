//! Linear-scan workspace allocation. An output cannot reuse either input until
//! its producing dispatch finishes; stream ordering then permits exact reuse.
use super::graph::Graph;
use anyhow::{ensure, Context, Result};

#[derive(Debug)]
pub(super) struct Layout {
    pub slots: Vec<usize>,
    pub values: Vec<Option<usize>>,
}
impl Layout {
    pub fn plan(graph: &Graph) -> Result<Self> {
        let mut result = Self {
            slots: vec![],
            values: vec![None; graph.lengths.len()],
        };
        let mut last = vec![None; graph.lengths.len()];
        for (i, step) in graph.steps.iter().enumerate() {
            for &input in &step.inputs {
                ensure!(input < last.len(), "invalid CUDA value");
                last[input] = Some(i);
            }
        }
        for &(value, _) in &graph.outputs {
            ensure!(value < last.len(), "invalid CUDA result");
            last[value] = Some(graph.steps.len());
        }
        // Inputs and weights stay live between predictions; workspace slots
        // alone are eligible for reuse. Unused shape constants allocate nothing.
        for (i, used) in last.iter().enumerate() {
            if used.is_some() && (i == graph.input || graph.constants.contains_key(&i)) {
                result.values[i] = Some(result.slots.len());
                result.slots.push(graph.lengths[i]);
            }
        }
        let mut retiring = vec![Vec::new(); graph.steps.len()];
        for (value, &end) in last.iter().enumerate() {
            if let Some(end) = end.filter(|&end| end < graph.steps.len()) {
                if value != graph.input && !graph.constants.contains_key(&value) {
                    retiring[end].push(value);
                }
            }
        }
        let mut free = Vec::<usize>::new();
        for (i, step) in graph.steps.iter().enumerate() {
            ensure!(
                step.inputs.iter().all(|&v| result.values[v].is_some()),
                "CUDA value has no storage"
            );
            let len = graph.lengths[step.output];
            let slot = if let Some((at, _)) = free
                .iter()
                .enumerate()
                .filter(|(_, s)| result.slots[**s] >= len)
                .min_by_key(|(_, s)| result.slots[**s])
            {
                free.swap_remove(at)
            } else {
                let slot = result.slots.len();
                result.slots.push(len);
                slot
            };
            result.values[step.output] = Some(slot);
            // Retire every value exactly once even if used in both operands.
            for &v in &retiring[i] {
                free.push(result.values[v].context("unallocated CUDA value")?);
            }
            if last[step.output].is_none() {
                free.push(slot);
            }
        }
        Ok(result)
    }
    pub fn bytes(&self) -> Result<usize> {
        self.slots
            .iter()
            .try_fold(0usize, |sum, &n| sum.checked_add(n.checked_mul(4)?))
            .ok_or_else(|| anyhow::anyhow!("CUDA workspace overflow"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cuda::graph::Step;
    #[test]
    fn branches_and_outputs_remain_live_while_dead_workspaces_are_reused() {
        let step = |inputs, output| Step {
            inputs,
            output,
            params: vec![0],
            kernel: 0,
            blocks: 1,
        };
        let g = Graph {
            constants: Default::default(),
            lengths: vec![8; 6],
            input: 0,
            input_shape: vec![8],
            outputs: vec![(5, vec![8])],
            steps: vec![
                step(vec![0], 1),
                step(vec![1], 2),
                step(vec![1, 2], 3),
                step(vec![3], 4),
                step(vec![4], 5),
            ],
        };
        let l = Layout::plan(&g).unwrap();
        assert_ne!(l.values[1], l.values[2]);
        assert_ne!(l.values[1], l.values[3]);
        assert_ne!(l.values[2], l.values[3]);
        assert!(l.slots.len() < g.lengths.len());
        for s in &g.steps {
            for &v in &s.inputs {
                assert_ne!(l.values[v], l.values[s.output]);
            }
        }
    }
}
