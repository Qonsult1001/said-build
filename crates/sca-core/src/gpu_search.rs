//! GPU-accelerated Hamming distance search via wgpu compute shader.
//!
//! Sends 64-bit fingerprints to the GPU, runs XOR + popcount on all
//! documents simultaneously, returns sorted results in milliseconds.
//!
//! Works on: NVIDIA (Vulkan), AMD (Vulkan), Apple (Metal), Intel (DX12).
//! Fallback: CPU Hamming scan (rayon parallel) when no GPU available.
//!
//! Usage:
//!   let gpu = GpuHammingSearch::new().await?;
//!   gpu.upload_index(&fingerprints).await;
//!   let results = gpu.search(&query_fingerprint, top_k).await;

use std::sync::Arc;

/// A 64-bit fingerprint stored as two u32s (GPU-friendly).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Fingerprint64 {
    pub lo: u32,
    pub hi: u32,
}

impl Fingerprint64 {
    /// Create from an 8-byte slice (1-bit quantized embedding).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 8);
        Self {
            lo: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            hi: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        }
    }
}

/// GPU Hamming search engine.
pub struct GpuHammingSearch {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    /// Uploaded document fingerprints (on GPU memory).
    doc_buffer: Option<wgpu::Buffer>,
    doc_count: usize,
}

impl GpuHammingSearch {
    /// Initialize the GPU compute pipeline.
    /// Returns None if no GPU is available (fallback to CPU).
    pub async fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }).await?;

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("SCA Hamming GPU"),
                ..Default::default()
            },
            None,
        ).await.ok()?;

        // Load the WGSL shader
        let shader_source = include_str!("gpu_hamming.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hamming_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // Bind group layout: query (0), docs (1), results (2)
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hamming_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hamming_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("hamming_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        eprintln!("[SCA-GPU] Initialized: {}", adapter.get_info().name);

        Some(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            doc_buffer: None,
            doc_count: 0,
        })
    }

    /// Upload document fingerprints to GPU memory.
    /// Call once after indexing, then search many times.
    pub fn upload_index(&mut self, fingerprints: &[Fingerprint64]) {
        use wgpu::util::DeviceExt;
        let bytes: &[u8] = bytemuck::cast_slice(fingerprints);
        self.doc_buffer = Some(self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("doc_fingerprints"),
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        }));
        self.doc_count = fingerprints.len();
    }

    /// Search: compute Hamming distance from query to all docs on GPU.
    /// Returns (doc_index, hamming_distance) sorted by distance (best first).
    pub async fn search(&self, query: &Fingerprint64, top_k: usize) -> Vec<(usize, u32)> {
        use wgpu::util::DeviceExt;

        let doc_buffer = match &self.doc_buffer {
            Some(b) => b,
            None => return vec![],
        };

        // Query buffer (8 bytes)
        let query_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("query"),
            contents: bytemuck::bytes_of(query),
            usage: wgpu::BufferUsages::STORAGE,
        });

        // Results buffer (4 bytes per doc)
        let results_size = (self.doc_count * 4) as u64;
        let results_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("results"),
            size: results_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Staging buffer for readback
        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: results_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hamming_bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: query_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: doc_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: results_buffer.as_entire_binding() },
            ],
        });

        // Dispatch compute
        let workgroups = (self.doc_count as u32 + 63) / 64;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hamming_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("hamming_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        // Copy results to staging
        encoder.copy_buffer_to_buffer(&results_buffer, 0, &staging_buffer, 0, results_size);
        self.queue.submit(std::iter::once(encoder.finish()));

        // Read back results
        let buffer_slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();

        let data = buffer_slice.get_mapped_range();
        let distances: &[u32] = bytemuck::cast_slice(&data);

        // Sort by distance, return top_k
        let mut results: Vec<(usize, u32)> = distances.iter()
            .enumerate()
            .map(|(i, &d)| (i, d))
            .collect();
        results.sort_by_key(|&(_, d)| d);
        results.truncate(top_k);

        drop(data);
        staging_buffer.unmap();

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_from_bytes() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let fp = Fingerprint64::from_bytes(&bytes);
        assert_eq!(fp.lo, 0x04030201);
        assert_eq!(fp.hi, 0x08070605);
    }
}
