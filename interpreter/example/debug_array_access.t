fn main() -> i64 {
    val a: [i64; 1] = [1i64]  # 明示的にi64を指定
    a[0u64]  # この値がi64であることを期待
}