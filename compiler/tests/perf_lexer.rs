//! 词法分析性能基准（P1.11 — 扫描速度 > 10 MB/s）
//!
//! - 默认（`cargo test`，debug 构建）只打印吞吐率，不做硬断言
//!   （debug 构建未优化，阈值不可靠）
//! - 发布构建（`cargo test --release`）下断言吞吐率 > 10 MB/s
//!
//! 运行方式：
//! ```text
//! cargo test --test perf_lexer -- --nocapture          # 查看吞吐率
//! cargo test --release --test perf_lexer -- --nocapture # 校验吞吐率达标
//! ```

use compiler::{Lexer, TokenKind};
use std::time::Instant;

/// 构造近似真实代码的源码（声明、表达式、字符串、注释混合）
fn build_source(target_bytes: usize) -> String {
    let chunk = concat!(
        "val counter: Int = 42 // a simple comment\n",
        "fun add(a: Int, b: Int): Int { return a + b }\n",
        "struct Point(val x: Float, val y: Float)\n",
        "val message = \"hello $name and ${a + b}\"\n",
        "/* block comment spanning\n   multiple lines */\n",
        "for (i in 0..100) { if (i % 2 == 0) continue }\n",
    );
    let mut src = String::with_capacity(target_bytes);
    while src.len() < target_bytes {
        src.push_str(chunk);
    }
    src.truncate(target_bytes);
    src
}

/// 扫描 `bytes` 字节源码，返回吞吐率（MB/s）
fn measure_throughput(bytes: usize) -> f64 {
    let source = build_source(bytes);
    let mut lexer = Lexer::new(&source);

    let start = Instant::now();
    let mut count = 0usize;
    loop {
        let tok = lexer.next_token();
        count += 1;
        if tok.kind == TokenKind::EOF {
            break;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();

    let mbps = (bytes as f64 / 1_048_576.0) / elapsed;
    println!(
        "scanned {} bytes / {} tokens in {:.3}s -> {:.2} MB/s",
        bytes, count, elapsed, mbps
    );
    mbps
}

#[test]
fn lexer_throughput_meets_target() {
    let bytes = 2 * 1024 * 1024; // 2 MB
    let mbps = measure_throughput(bytes);

    // release 构建下校验指标；debug 构建仅输出报告
    if !cfg!(debug_assertions) {
        assert!(
            mbps > 10.0,
            "lexer throughput below target: {:.2} MB/s (expected > 10 MB/s)",
            mbps
        );
    }
}

#[test]
fn lexer_throughput_on_small_input() {
    // 小输入不应出现异常退化（仅冒烟，阈值宽松）
    let mbps = measure_throughput(128 * 1024);
    assert!(mbps > 0.0);
}
