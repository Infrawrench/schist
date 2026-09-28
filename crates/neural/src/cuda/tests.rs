use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn typed(bytes: &[u8], shape: &[usize]) -> Result<TypedModel> {
    let mut onnx = tract_onnx::onnx();
    crate::gather_nd::register(&mut onnx);
    crate::deform_sample::register(&mut onnx);
    let mut proto = onnx.proto_model_for_read(&mut std::io::Cursor::new(bytes))?;
    crate::gather_nd::specialize_pixel_indices(&mut proto);
    // These existing fixtures use Sin solely to force wgpu partitioning.
    // Identity lets CUDA's resident matrix compiler exercise their layouts.
    for n in &mut proto.graph.as_mut().unwrap().node {
        if n.op_type == "Sin" {
            n.op_type = "Identity".into();
        }
    }
    onnx.model_for_proto_model(&proto)?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_typed()
}
fn input(shape: &[usize]) -> Result<TValue> {
    let values = (0..shape.iter().product())
        .map(|i| ((i * 17 % 101) as f32 - 50.) / 61.)
        .collect::<Vec<_>>();
    Ok(Tensor::from_shape(shape, &values)?.into())
}
fn host(graph: &Graph, data: &TValue) -> Result<Vec<f32>> {
    let executable = std::env::var_os("SCHIST_CUDA_HOST_RUNNER")
        .context("run through make check-neural-cuda to build the host arithmetic checker")?;
    let layout = memory::Layout::plan(graph)?;
    let slot = |value: usize| layout.values[value].unwrap();
    let mut bytes = Vec::new();
    let mut put = |v: usize| bytes.extend_from_slice(&(v as u32).to_le_bytes());
    put(layout.slots.len());
    put(graph.constants.len() + 1);
    put(slot(graph.input));
    put(data.len());
    for v in data.to_plain_array_view::<f32>()? {
        put(v.to_bits() as usize);
    }
    for (&id, values) in &graph.constants {
        put(slot(id));
        put(values.len());
        for &v in values {
            put(v as usize);
        }
    }
    put(graph.steps.len());
    for s in &graph.steps {
        put(s.kernel);
        put(slot(s.output));
        put(graph.lengths[s.output]);
        put(s.inputs.len());
        for &i in &s.inputs {
            put(slot(i));
        }
        put(s.params.len());
        for &v in &s.params {
            put(v as usize);
        }
    }
    put(graph.outputs.len());
    for &(i, _) in &graph.outputs {
        put(slot(i));
    }
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(&bytes)?;
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        "CUDA host arithmetic checker failed (code {:?}): {}",
        output.status.code(),
        output.status
    );
    ensure!(output.stdout.len() % 4 == 0, "invalid host output");
    Ok(output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}
fn compare(actual: &[f32], expected: &TVec<TValue>) {
    let expected = expected
        .iter()
        .flat_map(|t| {
            t.to_plain_array_view::<f32>()
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let mut max = 0.0f32;
    for (i, (&a, &b)) in actual.iter().zip(&expected).enumerate() {
        max = max.max((a - b).abs());
        assert!(
            a.is_finite() && (a - b).abs() <= 3e-4 * (1. + b.abs()),
            "CUDA arithmetic at {i}: {a} != {b}"
        );
    }
    eprintln!("CUDA arithmetic: {} values, max error {max}", actual.len());
}
fn fixtures() -> Result<Vec<TypedModel>> {
    let mut models = Vec::new();
    for (name, shape) in [
        ("gpu-attention.onnx", [1, 3, 4, 5]),
        ("gpu-indexing.onnx", [1, 3, 4, 5]),
        ("gpu-partitioned-conv.onnx", [1, 3, 31, 27]),
        ("gpu-partitioned-wide.onnx", [1, 3, 31, 27]),
        ("gpu-matrix-transposed.onnx", [1, 3, 17, 19]),
        ("gpu-matrix-tails.onnx", [1, 3, 67, 71]),
        ("gpu-matrix-unit-axis.onnx", [1, 3, 17, 19]),
        ("gpu-matrix-broadcast.onnx", [1, 3, 17, 19]),
        ("gpu-matrix-relative.onnx", [1, 3, 17, 10]),
        ("gpu-matrix-reductions.onnx", [1, 3, 17, 19]),
    ] {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )?;
        models.push(typed(&bytes, &shape)?);
    }
    for (name, shape) in [
        ("cuda-resize-half_pixel.onnx", vec![2, 3, 4, 5]),
        ("cuda-resize-align_corners.onnx", vec![2, 3, 4, 5]),
        ("cuda-resize-asymmetric.onnx", vec![2, 3, 4, 5]),
        ("cuda-pad-slice.onnx", vec![2, 3, 4, 5]),
        ("cuda-deform.onnx", vec![2, 2, 3, 4, 5]),
        ("cuda-integer.onnx", vec![2, 3, 4, 5]),
        ("cuda-dynamic-gather.onnx", vec![2, 3, 4, 5]),
        ("cuda-softmax-2.onnx", vec![2, 3, 17, 19]),
        ("cuda-softmax-3.onnx", vec![2, 3, 4, 513]),
        ("cuda-grouped-conv.onnx", vec![2, 6, 9, 11]),
        ("cuda-cubic-0.75.onnx", vec![2, 3, 4, 5]),
        ("cuda-cubic-0.5.onnx", vec![2, 3, 4, 5]),
        ("cuda-restore-resize.onnx", vec![2, 3, 4, 5]),
        ("cuda-pool-0.onnx", vec![2, 3, 9, 11]),
        ("cuda-pool-1.onnx", vec![2, 3, 9, 11]),
        ("cuda-pool-global.onnx", vec![2, 3, 9, 11]),
        ("cuda-ReduceSum.onnx", vec![2, 3, 65, 67]),
        ("cuda-ReduceMax.onnx", vec![2, 3, 65, 67]),
        ("cuda-ReduceMin.onnx", vec![2, 3, 65, 67]),
        ("cuda-split-matrix.onnx", vec![2, 3, 65, 67]),
    ] {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )?;
        models.push(typed(&bytes, &shape)?);
    }
    let original = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models/matting.onnx.xz"),
    )?;
    models.push(typed(
        &crate::decode_model_bytes(&original)?,
        &[1, 4, 128, 128],
    )?);
    Ok(models)
}
#[test]
fn kernel_arithmetic_matches_tract_for_attention_indexing_and_convolutions() -> Result<()> {
    // Ordinary cargo test still exercises graph/memory validation; the Make
    // target additionally runs the standalone source-level arithmetic checker.
    for (i, model) in fixtures()?.into_iter().enumerate() {
        let graph = Graph::compile(&model).with_context(|| format!("fixture {i}"))?;
        memory::Layout::plan(&graph)?;
        if std::env::var_os("SCHIST_CUDA_HOST_RUNNER").is_some() {
            let data = input(&graph.input_shape)?;
            let actual = host(&graph, &data)?;
            // Compare the typed operator semantics. tract 0.23.5's optimizer
            // rewrites integer division by a positive power of two into a
            // signed shift, which changes negative nonmultiples' rounding.
            let expected = model.into_runnable()?.run(tvec![data])?;
            compare(&actual, &expected);
        }
    }
    Ok(())
}
#[test]
fn embedded_ptx_is_standalone_and_has_all_entry_points() {
    assert!(PTX.ends_with('\0'));
    assert!(PTX.contains(".target sm_75"));
    assert!(!PTX.contains(".extern .func"));
    for kernel in ["tensor", "convolution", "matrix", "softmax", "reduction"] {
        assert!(PTX.contains(&format!(".entry {kernel}(")));
    }
}
#[test]
fn unsupported_operations_decline_before_execution() -> Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gpu-pad-convtranspose.onnx");
    assert!(Graph::compile(&typed(&std::fs::read(path)?, &[1, 3, 4, 5])?).is_err());
    Ok(())
}
#[test]
fn invalid_dynamic_indices_and_integer_overflow_are_rejected() -> Result<()> {
    if std::env::var_os("SCHIST_CUDA_HOST_RUNNER").is_none() {
        return Ok(());
    }
    for name in ["cuda-dynamic-gather.onnx", "cuda-integer.onnx"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let graph = Graph::compile(&typed(&std::fs::read(path)?, &[2, 3, 4, 5])?)?;
        for invalid in [
            f32::NAN,
            f32::INFINITY,
            16_777_216.,
            -16_777_216.,
            16_777_216. / 14.,
        ] {
            let data = Tensor::from_shape(&[2, 3, 4, 5], &vec![invalid; 120])?.into();
            let error = host(&graph, &data).unwrap_err();
            assert!(error.to_string().contains("code Some(3)"), "{error:#}");
        }
    }
    Ok(())
}
#[test]
#[ignore = "requires a real NVIDIA GPU; failure is never counted as fallback success"]
fn cuda_hardware_matches_cpu() -> Result<()> {
    for model in fixtures()? {
        let graph = Graph::compile(&model)?;
        let data = input(&graph.input_shape)?;
        let network =
            Network::load("matting", &model).context("CUDA unavailable or graph declined")?;
        assert!(network.active());
        let expected = model.into_runnable()?.run(tvec![data.clone()])?;
        // Repeated and concurrent callers check context migration and reuse.
        std::thread::scope(|scope| -> Result<()> {
            let jobs = (0..3)
                .map(|_| {
                    scope.spawn(|| -> Result<()> {
                        let out = network
                            .run(&tvec![data.clone()])
                            .context("CUDA failed; fallback is forbidden in this test")?;
                        let flat = out
                            .iter()
                            .flat_map(|v| {
                                v.to_plain_array_view::<f32>()
                                    .unwrap()
                                    .iter()
                                    .copied()
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>();
                        compare(&flat, &expected);
                        Ok(())
                    })
                })
                .collect::<Vec<_>>();
            for job in jobs {
                job.join().unwrap()?;
            }
            Ok(())
        })?;
    }
    Ok(())
}
#[test]
#[ignore = "reads maintainer model weights, but requires no GPU"]
fn all_background_models_compile_to_resident_cuda_graphs() -> Result<()> {
    let mut errors = Vec::new();
    for (id, cropped) in [
        ("matting", false),
        ("subject-guide", false),
        ("detail-matting", false),
        ("detail-matting", true),
        ("foreground", false),
        ("foreground-matting", false),
    ] {
        let model = background_model(id, cropped)?;
        let graph = match Graph::compile(&model) {
            Ok(graph) => graph,
            Err(error) => {
                eprintln!("CUDA {id}: {error:#}");
                errors.push(format!("{id}: {error:#}"));
                continue;
            }
        };
        let layout = memory::Layout::plan(&graph)?;
        eprintln!(
            "CUDA {id} cropped={cropped}: {} dispatches, {:.1} MiB resident storage",
            graph.steps.len(),
            layout.bytes()? as f64 / 1048576.
        );
    }
    ensure!(
        errors.is_empty(),
        "CUDA graph failures: {}",
        errors.join("; ")
    );
    Ok(())
}

#[test]
#[ignore = "requires NVIDIA hardware and installed maintainer weights; runs full CPU references"]
fn cuda_hardware_background_models_match_cpu() -> Result<()> {
    for (id, cropped) in [
        ("anti-smudge", false),
        ("matting", false),
        ("subject-guide", false),
        ("detail-matting", false),
        ("detail-matting", true),
        ("foreground", false),
        ("foreground-matting", false),
    ] {
        let model = background_model(id, cropped)?;
        let graph = Graph::compile(&model)?;
        let data = input(&graph.input_shape)?;
        drop(graph);
        let network = Network::load(id, &model).context("CUDA unavailable or graph declined")?;
        let expected = crate::cpu_threads::run(id, || {
            model
                .into_optimized()?
                .into_runnable()?
                .run(tvec![data.clone()])
        })?;
        for repeat in 0..2 {
            let start = std::time::Instant::now();
            let output = network.run(&tvec![data.clone()]).context("CUDA failed")?;
            eprintln!(
                "CUDA {id} cropped={cropped} repeat={repeat}: {:?}",
                start.elapsed()
            );
            let flat = output
                .iter()
                .flat_map(|v| {
                    v.to_plain_array_view::<f32>()
                        .unwrap()
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            compare(&flat, &expected);
        }
    }
    Ok(())
}

fn background_model(id: &str, cropped: bool) -> Result<TypedModel> {
    let spec = crate::spec(id).unwrap();
    let path = if spec.file.ends_with(".xz") {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join(spec.file)
    } else {
        crate::model_dir().join(spec.file)
    };
    let original = std::fs::read(path)?;
    let bytes = crate::decode_model_bytes(&original)?;
    let mut onnx = tract_onnx::onnx();
    crate::gather_nd::register(&mut onnx);
    crate::deform_sample::register(&mut onnx);
    let mut proto = onnx.proto_model_for_read(&mut std::io::Cursor::new(bytes.as_ref()))?;
    if cropped {
        crate::detail_crop::optimize(&mut proto)?;
    }
    if matches!(id, "foreground" | "foreground-matting") {
        crate::gather_nd::specialize_pixel_indices(&mut proto);
        crate::deform_sample::optimize(&mut proto);
    }
    proto.graph.as_mut().unwrap().value_info.clear();
    let (w, h) = spec.input.dims();
    let c = if matches!(id, "matting" | "detail-matting") {
        4
    } else {
        3
    };
    onnx.model_for_proto_model(&proto)?
        .with_input_fact(0, f32::fact([1, c, h, w]).into())?
        .into_typed()
}

#[test]
#[ignore = "compiles the shipped restoration graph; no NVIDIA device required"]
fn anti_smudge_compiles_to_resident_cuda_graph() -> Result<()> {
    let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
    let model = typed(&bytes, &[1, 3, 2048, 2048])?;
    let graph = Graph::compile(&model)?;
    assert!(graph.steps.iter().any(|s| s.kernel == 4));
    assert!(graph.steps.iter().any(|s| s.params[0] == 15));
    let layout = memory::Layout::plan(&graph)?;
    eprintln!(
        "CUDA anti-smudge: {} dispatches, {:.1} MiB resident storage",
        graph.steps.len(),
        layout.bytes()? as f64 / 1048576.
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "runs the full 2048 restoration model twice on CPU"]
fn anti_smudge_accelerated_cpu_matches_original() -> Result<()> {
    let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
    let model = typed(&bytes, &[1, 3, 2048, 2048])?;
    let data = Tensor::from_shape(
        &[1, 3, 2048, 2048],
        &(0..3 * 2048 * 2048)
            .map(|i| ((i * 17 % 101) as f32) / 100.)
            .collect::<Vec<_>>(),
    )?;
    let mut reference = None;
    for id in ["reference", "anti-smudge"] {
        let start = std::time::Instant::now();
        let plan = crate::prepare_cpu_plan(id, model.clone(), false)?;
        eprintln!("{id}: CPU preparation {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let output = plan.run(tvec!(data.clone().into()))?;
        eprintln!("{id}: CPU inference {:?}", start.elapsed());
        if let Some(expected) = &reference {
            let actual = output[0].to_plain_array_view::<f32>()?;
            compare(actual.as_slice().context("noncontiguous output")?, expected);
        } else {
            reference = Some(output);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "full restoration operator timings; make profile-anti-smudge"]
fn profile_anti_smudge_cpu() -> Result<()> {
    let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
    let model = typed(&bytes, &[1, 3, 2048, 2048])?;
    let plan = crate::prepare_cpu_plan("anti-smudge", model, false)?;
    let data = input(&[1, 3, 2048, 2048])?;
    let mut totals = std::collections::HashMap::<String, (usize, f64)>::new();
    let mut nodes = Vec::new();
    let start = std::time::Instant::now();
    let mut state = plan.spawn()?;
    let output = crate::cpu_threads::run("anti-smudge", || {
        state.run_plan_with_eval(tvec!(data), |state, op_state, node, inputs| {
            let shapes = inputs
                .iter()
                .map(|v| v.shape().to_vec())
                .collect::<Vec<_>>();
            let start = std::time::Instant::now();
            let result = tract_onnx::tract_core::plan::eval(state, op_state, node, inputs);
            let seconds = start.elapsed().as_secs_f64();
            let kind = node.op().name().into_owned();
            let entry = totals.entry(kind.clone()).or_default();
            entry.0 += 1;
            entry.1 += seconds;
            nodes.push((seconds, kind, node.name.clone(), shapes));
            result
        })
    })?;
    std::hint::black_box(output);
    eprintln!("Restoration total: {:.3}s", start.elapsed().as_secs_f64());
    let mut totals = totals.into_iter().collect::<Vec<_>>();
    totals.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    for (op, (calls, seconds)) in totals {
        eprintln!("{seconds:.6}s {calls} {op}");
    }
    nodes.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (seconds, op, name, shapes) in nodes.into_iter().take(50) {
        eprintln!("{seconds:.6}s {op} {name} {shapes:?}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "paired full-model restoration kernel timing; make profile-anti-smudge-kernels"]
fn anti_smudge_row_kernels_match_previous_cpu() -> Result<()> {
    let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
    let model = typed(&bytes, &[1, 3, 2048, 2048])?;
    let data: TValue = Tensor::from_shape(
        &[1, 3, 2048, 2048],
        &(0..3 * 2048 * 2048)
            .map(|i| (i * 17 % 101) as f32 / 100.)
            .collect::<Vec<_>>(),
    )?
    .into();
    let before = crate::prepare_cpu_plan_with_options(
        "anti-smudge",
        model.clone(),
        false,
        crate::CpuOptimizations {
            restoration_kernels: false,
            tiled_attention: false,
            image_rows: false,
        },
    )?;
    let after = crate::prepare_cpu_plan_with_options(
        "anti-smudge",
        model,
        false,
        crate::CpuOptimizations {
            restoration_kernels: true,
            tiled_attention: false,
            image_rows: false,
        },
    )?;
    let mut expected: Option<TVec<TValue>> = None;
    let mut times = [Vec::new(), Vec::new()];
    let mut max_difference = 0.0f32;
    for round in 0..4 {
        for slot in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
            let start = std::time::Instant::now();
            let output = crate::cpu_threads::run("anti-smudge", || {
                if slot == 0 { &before } else { &after }.run(tvec!(data.clone()))
            })?;
            let seconds = start.elapsed().as_secs_f64();
            eprintln!(
                "restoration kernels round={round} optimized={}: {seconds:.3}s",
                slot == 1
            );
            if round != 0 {
                times[slot].push(seconds);
            }
            if let Some(reference) = &expected {
                let reference = reference[0].to_plain_array_view::<f32>()?;
                let actual = output[0].to_plain_array_view::<f32>()?;
                assert_eq!(actual.shape(), reference.shape());
                for (&a, &b) in actual.iter().zip(reference.iter()) {
                    let difference = (a - b).abs();
                    assert!(
                        a.is_finite() && b.is_finite() && difference < 5e-5,
                        "restoration output {a} != {b}"
                    );
                    max_difference = max_difference.max(difference);
                }
            } else {
                expected = Some(output);
            }
        }
    }
    for values in &mut times {
        values.sort_by(f64::total_cmp);
    }
    eprintln!("Restoration kernels paired medians (3 warm runs each): {:.3}s -> {:.3}s; max absolute difference {max_difference}", times[0][1], times[1][1]);
    Ok(())
}
