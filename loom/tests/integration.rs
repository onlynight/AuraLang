//! [Phase B3] 构建缓存集成测试
//!
//! 端到端测试两级缓存架构（本地 + 远程）、增量构建、缓存失效。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use aura_loom::cache::fingerprint::Fingerprint;
use aura_loom::cache::local::LocalCache;
use aura_loom::cache::remote::{CacheHitSource, CacheService, RemoteCacheConfig};
use aura_loom::lifecycle::phases::build_standard_task_graph;
use aura_loom::manifest::parse::default_manifest;
use aura_loom::manifest::priority::ResolvedBuildConfig;
use aura_loom::task::scheduler::{Scheduler, SchedulerConfig};
use aura_loom::task::{TaskDefinition, TaskGraph, TaskInputs, TaskKind, TaskOutputs};

// ═══════════════════════════════════════════════════════════════════════════════
// 辅助函数
// ═══════════════════════════════════════════════════════════════════════════════

fn setup_project(tmp: &TempDir) -> PathBuf {
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.aura"), "fun main() { println(\"hello\") }").unwrap();
    std::fs::write(
        src.join("utils.aura"),
        "fun add(a: Int, b: Int): Int { a + b }",
    )
    .unwrap();
    tmp.path().to_path_buf()
}

/// 使用**独立临时目录**的构建配置。
///
/// `isolated_config()` 的 `out_dir` / `cache_dir` 是相对路径
/// （`target/build`、`target/cache`），`cargo test` 的 CWD 是包目录 —— 多个用例
/// 并行执行会共用同一目录并互相删除/覆盖（`clean` 任务直接 `remove_dir_all`），
/// 表现为 `test_build_after_source_change` 等用例随机失败。
fn isolated_config() -> ResolvedBuildConfig {
    let dir = TempDir::new().unwrap().into_path();
    ResolvedBuildConfig {
        out_dir: dir.join("build").to_string_lossy().to_string(),
        cache_dir: dir.join("cache").to_string_lossy().to_string(),
        ..Default::default()
    }
}

fn make_cache_service(
    cache_dir: &std::path::Path,
    build_config: Arc<ResolvedBuildConfig>,
) -> Result<Arc<Mutex<CacheService>>, aura_loom::LoomError> {
    let local = Arc::new(Mutex::new(LocalCache::new(cache_dir)?));
    let service = CacheService::new(local, None, build_config)?;
    Ok(Arc::new(Mutex::new(service)))
}

fn make_task_with_files(name: &str, kind: TaskKind, files: &[PathBuf]) -> TaskDefinition {
    TaskDefinition {
        name: name.to_string(),
        description: format!("test task {}", name),
        kind,
        depends_on: Vec::new(),
        inputs: TaskInputs {
            files: files.to_vec(),
            options: std::collections::HashMap::new(),
            dep_fingerprints: std::collections::HashMap::new(),
        },
        outputs: TaskOutputs::default(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.1: 本地缓存 Artifact 管理集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_local_cache_artifact_lifecycle() {
    let tmp = TempDir::new().unwrap();

    // 1. 创建缓存
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    // 2. 存储 fingerprint
    cache.store_fingerprint("compile-main", "fp123").unwrap();

    // 3. 创建并存储产物
    let file1 = tmp.path().join("main.auc");
    std::fs::write(&file1, "compiled bytecode 1").unwrap();
    let file2 = tmp.path().join("utils.auc");
    std::fs::write(&file2, "compiled bytecode 2").unwrap();

    let entries = cache
        .store_artifacts(
            "compile-main",
            &[
                file1, file2,
            ],
        )
        .unwrap();
    assert_eq!(entries.len(), 2);

    // 4. 验证缓存状态
    assert!(cache.is_up_to_date("compile-main", "fp123"));
    assert!(!cache.is_up_to_date("compile-main", "fp999"));
    assert!(cache.has_artifacts("compile-main"));
    assert!(cache.can_restore("compile-main"));

    // 5. 恢复产物
    let restore_dir = tmp.path().join("restored");
    let restored = cache.restore_artifacts("compile-main", &restore_dir).unwrap();
    assert_eq!(restored.len(), 2);
    assert!(restored.iter().all(|p| p.exists()));

    // 6. 验证恢复的内容
    let content1 = std::fs::read_to_string(&restore_dir.join("main.auc")).unwrap();
    let content2 = std::fs::read_to_string(&restore_dir.join("utils.auc")).unwrap();
    assert_eq!(content1, "compiled bytecode 1");
    assert_eq!(content2, "compiled bytecode 2");

    // 7. 清除缓存
    cache.clear().unwrap();
    assert!(!cache.has_artifacts("compile-main"));
    assert!(cache.lookup_fingerprint("compile-main").unwrap().is_none());
}

#[test]
fn test_local_cache_metadata_persistence() {
    let tmp = TempDir::new().unwrap();

    // 第一次会话：存储数据
    {
        let mut cache = LocalCache::new(tmp.path()).unwrap();
        cache.store_fingerprint("compile-main", "fp1").unwrap();
        cache.store_fingerprint("package", "fp2").unwrap();

        let file = tmp.path().join("app.auz");
        std::fs::write(&file, "package data").unwrap();
        cache.store_artifacts("package", &[file]).unwrap();
    }

    // 第二次会话：重新加载
    {
        let cache = LocalCache::new(tmp.path()).unwrap();
        assert!(cache.is_up_to_date("compile-main", "fp1"));
        assert!(cache.is_up_to_date("package", "fp2"));
        assert!(cache.has_artifacts("package"));

        let entries = cache.list_artifacts("package").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.contains("app.auz"));
    }
}

#[test]
fn test_local_cache_multiple_tasks() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    // 存储多个任务的产物
    let file1 = tmp.path().join("main.auc");
    std::fs::write(&file1, "compile output").unwrap();
    cache.store_artifacts("compile-main", &[file1.clone()]).unwrap();

    let file2 = tmp.path().join("test.auc");
    std::fs::write(&file2, "test output").unwrap();
    cache.store_artifacts("compile-test", &[file2.clone()]).unwrap();

    let file3 = tmp.path().join("app.auz");
    std::fs::write(&file3, "package output").unwrap();
    cache.store_artifacts("package", &[file3.clone()]).unwrap();

    // 验证各任务的产物独立
    assert!(cache.has_artifacts("compile-main"));
    assert!(cache.has_artifacts("compile-test"));
    assert!(cache.has_artifacts("package"));

    let entries1 = cache.list_artifacts("compile-main").unwrap();
    let entries2 = cache.list_artifacts("compile-test").unwrap();
    let entries3 = cache.list_artifacts("package").unwrap();

    assert_eq!(entries1.len(), 1);
    assert_eq!(entries2.len(), 1);
    assert_eq!(entries3.len(), 1);

    // 验证哈希不同（不同内容）
    assert!(entries1[0].hash != entries2[0].hash);
    assert!(entries1[0].hash != entries3[0].hash);

    // 删除一个任务的产物
    cache.remove_task_artifacts("compile-test").unwrap();
    assert!(!cache.has_artifacts("compile-test"));
    assert!(cache.has_artifacts("compile-main"));
    assert!(cache.has_artifacts("package"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.2: 缓存失效策略集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cache_invalidation_on_source_change() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    // 创建源文件
    let src_file = tmp.path().join("src/main.aura");
    let src_parent = src_file.parent().unwrap();
    std::fs::create_dir_all(src_parent).unwrap();
    std::fs::write(&src_file, "fun main() { println(\"v1\") }").unwrap();

    // 第一次构建：计算 fingerprint 并存储
    let task1 = make_task_with_files(
        "compile-main",
        TaskKind::Compile("main".to_string()),
        &[src_file.clone()],
    );
    let fp1 = Fingerprint::compute(&task1).unwrap();
    cache.store_fingerprint("compile-main", &fp1).unwrap();

    let artifact = tmp.path().join("main.auc");
    std::fs::write(&artifact, "bytecode v1").unwrap();
    cache.store_artifacts("compile-main", &[artifact]).unwrap();

    // 验证 up-to-date
    assert!(cache.is_up_to_date("compile-main", &fp1));

    // 修改源文件
    std::fs::write(&src_file, "fun main() { println(\"v2\") }").unwrap();

    // 第二次构建：fingerprint 应该变化
    let task2 = make_task_with_files(
        "compile-main",
        TaskKind::Compile("main".to_string()),
        &[src_file.clone()],
    );
    let fp2 = Fingerprint::compute(&task2).unwrap();
    assert_ne!(fp1, fp2);

    // 验证 not up-to-date
    assert!(!cache.is_up_to_date("compile-main", &fp2));

    // 更新缓存
    cache.store_fingerprint("compile-main", &fp2).unwrap();
    assert!(cache.is_up_to_date("compile-main", &fp2));
}

#[test]
fn test_cache_invalidation_on_option_change() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    let src_file = tmp.path().join("main.aura");
    std::fs::write(&src_file, "fun main() {}").unwrap();

    // opt-level = 2
    let task1 = TaskDefinition {
        name: "compile-main".to_string(),
        description: String::new(),
        kind: TaskKind::Compile("main".to_string()),
        depends_on: Vec::new(),
        inputs: TaskInputs {
            files: vec![src_file.clone()],
            options: [("opt-level".to_string(), "2".to_string())].iter().cloned().collect(),
            dep_fingerprints: std::collections::HashMap::new(),
        },
        outputs: TaskOutputs::default(),
    };
    let fp1 = Fingerprint::compute(&task1).unwrap();

    // opt-level = 3
    let task2 = TaskDefinition {
        name: "compile-main".to_string(),
        description: String::new(),
        kind: TaskKind::Compile("main".to_string()),
        depends_on: Vec::new(),
        inputs: TaskInputs {
            files: vec![src_file.clone()],
            options: [("opt-level".to_string(), "3".to_string())].iter().cloned().collect(),
            dep_fingerprints: std::collections::HashMap::new(),
        },
        outputs: TaskOutputs::default(),
    };
    let fp2 = Fingerprint::compute(&task2).unwrap();

    assert_ne!(fp1, fp2);
}

#[test]
fn test_cache_invalidation_on_dependency_change() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    // 测试依赖的 fingerprint 变化导致下游任务失效
    let deps_v1: std::collections::HashMap<String, String> =
        [("compile-main".to_string(), "abc123".to_string())].iter().cloned().collect();
    let deps_v2: std::collections::HashMap<String, String> =
        [("compile-main".to_string(), "def456".to_string())].iter().cloned().collect();

    let task1 = TaskDefinition {
        name: "run-tests".to_string(),
        description: String::new(),
        kind: TaskKind::Test,
        depends_on: vec!["compile-main".to_string()],
        inputs: TaskInputs {
            files: Vec::new(),
            options: std::collections::HashMap::new(),
            dep_fingerprints: deps_v1,
        },
        outputs: TaskOutputs::default(),
    };
    let fp1 = Fingerprint::compute(&task1).unwrap();

    let task2 = TaskDefinition {
        name: "run-tests".to_string(),
        description: String::new(),
        kind: TaskKind::Test,
        depends_on: vec!["compile-main".to_string()],
        inputs: TaskInputs {
            files: Vec::new(),
            options: std::collections::HashMap::new(),
            dep_fingerprints: deps_v2,
        },
        outputs: TaskOutputs::default(),
    };
    let fp2 = Fingerprint::compute(&task2).unwrap();

    assert_ne!(fp1, fp2);
}

#[test]
fn test_cache_clear_removes_everything() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    // 存储多个任务
    cache.store_fingerprint("compile-main", "fp1").unwrap();
    cache.store_fingerprint("compile-test", "fp2").unwrap();
    cache.store_fingerprint("package", "fp3").unwrap();

    let file = tmp.path().join("test.auz");
    std::fs::write(&file, "data").unwrap();
    cache.store_artifacts("package", &[file]).unwrap();

    assert_eq!(cache.all_fingerprints().len(), 3);
    assert!(cache.has_artifacts("package"));

    // 清除所有缓存
    cache.clear().unwrap();

    assert_eq!(cache.all_fingerprints().len(), 0);
    assert!(!cache.has_artifacts("package"));
    assert!(!cache.is_up_to_date("compile-main", "fp1"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.4: CacheService 两级缓存集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cache_service_local_hit() {
    let tmp = TempDir::new().unwrap();
    let build_config = Arc::new(isolated_config());
    let service = make_cache_service(tmp.path(), build_config).unwrap();
    let mut svc = service.lock().unwrap();

    let task = make_task_with_files("compile-main", TaskKind::Compile("main".to_string()), &[]);

    // 第一次：缓存未命中
    let (hit, source) = svc.lookup(&task).unwrap();
    assert!(!hit);
    assert_eq!(source, CacheHitSource::Miss);

    // 存储
    let artifact = tmp.path().join("main.auc");
    std::fs::write(&artifact, "bytecode").unwrap();
    svc.store(&task, &[artifact]).unwrap();

    // 第二次：本地缓存命中
    let (hit, source) = svc.lookup(&task).unwrap();
    assert!(hit);
    assert_eq!(source, CacheHitSource::Local);

    // 验证统计
    let stats = svc.stats();
    assert_eq!(stats.local_hits, 1);
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_cache_service_restore_from_cache() {
    let tmp = TempDir::new().unwrap();
    let build_config = Arc::new(isolated_config());
    let service = make_cache_service(tmp.path(), build_config).unwrap();
    let mut svc = service.lock().unwrap();

    let task = make_task_with_files("package", TaskKind::Package, &[]);

    // 存储
    let artifact = tmp.path().join("app.auz");
    std::fs::write(&artifact, "package data").unwrap();
    svc.store(&task, &[artifact]).unwrap();

    // 恢复到新目录
    let dest = tmp.path().join("restored");
    let restored = svc.restore(&task, &dest).unwrap();
    assert!(restored.is_some());
    let files = restored.unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].exists());

    let content = std::fs::read_to_string(&files[0]).unwrap();
    assert_eq!(content, "package data");
}

#[test]
fn test_cache_service_invalidate() {
    let tmp = TempDir::new().unwrap();
    let build_config = Arc::new(isolated_config());
    let service = make_cache_service(tmp.path(), build_config).unwrap();
    let mut svc = service.lock().unwrap();

    let task = make_task_with_files("compile-main", TaskKind::Compile("main".to_string()), &[]);

    // 存储
    let artifact = tmp.path().join("main.auc");
    std::fs::write(&artifact, "data").unwrap();
    svc.store(&task, &[artifact]).unwrap();

    // 验证缓存存在
    let (hit, _) = svc.lookup(&task).unwrap();
    assert!(hit);

    // 失效
    svc.invalidate(&task).unwrap();

    // 验证缓存已清除
    let (hit, _) = svc.lookup(&task).unwrap();
    assert!(!hit);
}

#[test]
fn test_cache_service_fingerprint_change() {
    let tmp = TempDir::new().unwrap();
    let build_config = Arc::new(isolated_config());
    let service = make_cache_service(tmp.path(), build_config).unwrap();
    let mut svc = service.lock().unwrap();

    let src_file = tmp.path().join("main.aura");
    std::fs::write(&src_file, "fun main() { println(\"v1\") }").unwrap();

    let task_v1 = make_task_with_files(
        "compile-main",
        TaskKind::Compile("main".to_string()),
        &[src_file.clone()],
    );

    // 存储 v1
    let artifact = tmp.path().join("main.auc");
    std::fs::write(&artifact, "bytecode v1").unwrap();
    svc.store(&task_v1, &[artifact]).unwrap();

    // v1 应该命中
    let (hit, _) = svc.lookup(&task_v1).unwrap();
    assert!(hit);

    // 修改源文件
    std::fs::write(&src_file, "fun main() { println(\"v2\") }").unwrap();

    let task_v2 = make_task_with_files(
        "compile-main",
        TaskKind::Compile("main".to_string()),
        &[src_file.clone()],
    );

    // v2 应该未命中（fingerprint 变了）
    let (hit, _) = svc.lookup(&task_v2).unwrap();
    assert!(!hit);
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.5: --no-cache / --clean 集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_scheduler_no_cache() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let graph = build_standard_task_graph(&manifest, &project_dir);
    let build_config = Arc::new(isolated_config());

    // no-cache 模式
    let config = SchedulerConfig {
        use_cache: false,
        ..Default::default()
    };
    let scheduler = Scheduler::new(
        Arc::new(graph),
        build_config.clone(),
        None, // 无缓存
        config,
    );

    // 执行 build
    let result = scheduler.execute("clean");
    assert!(result.is_ok());

    let results = scheduler.results();
    assert!(results.iter().all(|r| r.success));
    // no-cache 模式下没有缓存命中
    assert!(results.iter().all(|r| !r.cache_hit));
}

#[test]
fn test_scheduler_clean_mode() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let graph = build_standard_task_graph(&manifest, &project_dir);
    let build_config = Arc::new(isolated_config());

    // 创建缓存
    let cache_dir = tmp.path().join("target/cache");
    let cache = Arc::new(Mutex::new(LocalCache::new(&cache_dir).unwrap()));
    let service = Arc::new(Mutex::new(
        CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
    ));

    // 第一次：正常构建（缓存命中）
    let config1 = SchedulerConfig {
        use_cache: true,
        clean: false,
        ..Default::default()
    };
    let scheduler1 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config1,
    );
    assert!(scheduler1.execute("compile-main").is_ok());

    // 第二次：正常构建（应该缓存命中）
    let config2 = SchedulerConfig {
        use_cache: true,
        clean: false,
        ..Default::default()
    };
    let scheduler2 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config2,
    );
    assert!(scheduler2.execute("compile-main").is_ok());
    let results2 = scheduler2.results();
    assert!(results2.iter().any(|r| r.cache_hit));

    // 第三次：clean 模式（应该全部重新执行）
    let config3 = SchedulerConfig {
        use_cache: true,
        clean: true,
        ..Default::default()
    };
    let scheduler3 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config3,
    );
    assert!(scheduler3.execute("compile-main").is_ok());
    let results3 = scheduler3.results();
    assert!(results3.iter().all(|r| r.executed)); // clean 模式全部执行
}

#[test]
fn test_scheduler_cache_hit_on_second_run() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let build_config = Arc::new(isolated_config());

    let cache_dir = tmp.path().join("target/cache");
    let cache = Arc::new(Mutex::new(LocalCache::new(&cache_dir).unwrap()));
    let service = Arc::new(Mutex::new(
        CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
    ));

    // 第一次构建
    let config1 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler1 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config1,
    );
    assert!(scheduler1.execute("clean").is_ok());
    let results1 = scheduler1.results();
    let hits1 = results1.iter().filter(|r| r.cache_hit).count();

    // 第二次构建（应该缓存命中）
    let config2 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler2 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config2,
    );
    assert!(scheduler2.execute("clean").is_ok());
    let results2 = scheduler2.results();
    let hits2 = results2.iter().filter(|r| r.cache_hit).count();

    // 第二次构建的缓存命中数应该 >= 第一次
    assert!(hits2 >= hits1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.3: 远程缓存配置集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_remote_cache_config_from_manifest() {
    let build_config = ResolvedBuildConfig {
        cache_remote: Some("https://cache.aura-lang.dev".to_string()),
        cache_remote_shared: true,
        ..Default::default()
    };

    let config = RemoteCacheConfig::from_build_config(&build_config);
    assert!(config.is_some());
    let config = config.unwrap();
    assert_eq!(config.url, "https://cache.aura-lang.dev");
    assert!(config.shared);
    assert_eq!(config.timeout_secs, 30);
    assert_eq!(config.max_retries, 3);
}

#[test]
fn test_remote_cache_not_configured() {
    let build_config = isolated_config();
    assert!(RemoteCacheConfig::from_build_config(&build_config).is_none());
}

#[test]
fn test_remote_cache_cache_key_stability() {
    let task = make_task_with_files("compile-main", TaskKind::Compile("main".to_string()), &[]);
    let config = isolated_config();

    // 计算两次应该得到相同的 cache key
    let fp1 = Fingerprint::compute(&task).unwrap();
    let fp2 = Fingerprint::compute(&task).unwrap();
    assert_eq!(fp1, fp2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.6: 完整构建缓存流程集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_build_with_cache() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let build_config = Arc::new(isolated_config());

    let cache_dir = tmp.path().join("target/cache");
    let cache = Arc::new(Mutex::new(LocalCache::new(&cache_dir).unwrap()));
    let service = Arc::new(Mutex::new(
        CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
    ));

    // 第一次完整构建（clean → resolve → compile → package → verify → install）
    let config1 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler1 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config1,
    );
    assert!(scheduler1.execute("install").is_ok());

    let results1 = scheduler1.results();
    let executed1 = results1.iter().filter(|r| r.executed).count();
    let hits1 = results1.iter().filter(|r| r.cache_hit).count();

    // 第二次构建（大部分应该缓存命中）
    let config2 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler2 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config2,
    );
    assert!(scheduler2.execute("install").is_ok());

    let results2 = scheduler2.results();
    let executed2 = results2.iter().filter(|r| r.executed).count();
    let hits2 = results2.iter().filter(|r| r.cache_hit).count();

    // 第二次构建应该有更多缓存命中
    assert!(hits2 > hits1);
    // 第二次构建应该执行更少的任务
    assert!(executed2 <= executed1);

    // 验证所有任务都成功
    assert!(results2.iter().all(|r| r.success));
}

#[test]
fn test_build_after_source_change() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let build_config = Arc::new(isolated_config());

    let cache_dir = tmp.path().join("target/cache");
    let cache = Arc::new(Mutex::new(LocalCache::new(&cache_dir).unwrap()));
    let service = Arc::new(Mutex::new(
        CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
    ));

    // 第一次构建
    let config1 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler1 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config1,
    );
    assert!(scheduler1.execute("compile-main").is_ok());

    // 修改源文件
    let src_file = project_dir.join("src/main.aura");
    std::fs::write(&src_file, "fun main() { println(\"modified\") }").unwrap();

    // 第二次构建（compile-main 应该重新执行）
    let config2 = SchedulerConfig {
        use_cache: true,
        ..Default::default()
    };
    let scheduler2 = Scheduler::with_cache_service(
        Arc::new(build_standard_task_graph(&manifest, &project_dir)),
        build_config.clone(),
        Some(cache.clone()),
        Some(service.clone()),
        config2,
    );
    assert!(scheduler2.execute("compile-main").is_ok());

    let results2 = scheduler2.results();
    // compile-main 应该重新执行（缓存未命中）
    let compile_main = results2.iter().find(|r| r.task_name == "compile-main").unwrap();
    assert!(compile_main.executed);
    assert!(!compile_main.cache_hit);
}

#[test]
fn test_no_cache_always_executes() {
    let tmp = TempDir::new().unwrap();
    let project_dir = setup_project(&tmp);
    let manifest = default_manifest("test");

    let build_config = Arc::new(isolated_config());

    let cache_dir = tmp.path().join("target/cache");
    let cache = Arc::new(Mutex::new(LocalCache::new(&cache_dir).unwrap()));

    // no-cache 模式：每次都应该执行
    for i in 0..3 {
        let config = SchedulerConfig {
            use_cache: false,
            ..Default::default()
        };
        let scheduler = Scheduler::new(
            Arc::new(build_standard_task_graph(&manifest, &project_dir)),
            build_config.clone(),
            None,
            config,
        );
        assert!(scheduler.execute("clean").is_ok());

        let results = scheduler.results();
        assert!(
            results.iter().all(|r| !r.cache_hit),
            "iteration {} should have no cache hits",
            i
        );
    }
}

#[test]
fn test_cache_stats() {
    let tmp = TempDir::new().unwrap();
    let mut cache = LocalCache::new(tmp.path()).unwrap();

    cache.store_fingerprint("compile-main", "fp1").unwrap();
    cache.store_fingerprint("package", "fp2").unwrap();

    let file = tmp.path().join("app.auz");
    std::fs::write(&file, "package data").unwrap();
    cache.store_artifacts("package", &[file]).unwrap();

    let stats = cache.stats();
    assert_eq!(stats.fingerprint_count, 2);
    assert_eq!(stats.artifact_count, 1);
    assert!(stats.total_size_bytes > 0);
    assert_eq!(stats.version, "1.0");
}
