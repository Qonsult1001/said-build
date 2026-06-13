// SCA GPU Hamming Distance Shader
// 10 lines that replace an entire Vector Database
//
// Each document is a 64-bit fingerprint stored as vec2<u32>.
// XOR + popcount = Hamming distance in a single GPU cycle.
// A GTX 1060 runs this 1,280 times simultaneously.
// A modern GPU runs it 16,384 times simultaneously.

@group(0) @binding(0) var<storage, read> query: vec2<u32>;
@group(0) @binding(1) var<storage, read> docs: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read_write> results: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= arrayLength(&docs)) { return; }

    let xored = query ^ docs[i];
    results[i] = countOneBits(xored.x) + countOneBits(xored.y);
}
