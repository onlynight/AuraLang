//! 配置校验
//!
//! [Phase L1/B1.5] schema 校验 + 错误报告

use crate::manifest::LoomManifest;

/// 校验 Manifest，返回错误列表（空 = 通过）
pub fn validate_manifest(manifest: &LoomManifest) -> Vec<String> {
    let mut errors = Vec::new();

    // 1. 包名校验
    if manifest.name.is_empty() {
        errors.push("缺少必填字段: name".to_string());
    } else if !is_valid_package_name(&manifest.name) {
        errors.push(format!(
            "无效的包名: '{}'（仅允许小写字母、数字、连字符和点）",
            manifest.name
        ));
    }

    // 2. 版本号校验
    if manifest.version.is_empty() {
        errors.push("缺少必填字段: version".to_string());
    } else if !is_valid_version(&manifest.version) {
        errors.push(format!(
            "无效的版本号: '{}'（需符合 SemVer 格式）",
            manifest.version
        ));
    }

    // 3. 入口文件校验（仅应用包需要）
    if !manifest.library && manifest.entry.is_empty() {
        errors.push("应用包必须指定 entry 入口文件".to_string());
    }

    // 4. 构建配置校验
    if let Some(ref ws) = manifest.workspace {
        // Workspace 根配置不应有 entry
        if !manifest.entry.is_empty() {
            errors.push("Workspace 根配置不应指定 entry（entry 应在成员项目配置）".to_string());
        }
        if ws.members.is_empty() {
            errors.push("[workspace] 必须指定至少一个 members".to_string());
        }
        for member in &ws.members {
            if member.is_empty() {
                errors.push("[workspace] members 包含空路径".to_string());
            }
        }
    }

    // 5. Profile 校验
    let active_profiles: Vec<_> = manifest.profiles.iter().filter(|(_, p)| p.activate).collect();
    if active_profiles.len() > 1 {
        errors.push(format!(
            "最多只能有一个 profile 激活，当前激活: {}",
            active_profiles.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    // 6. 依赖校验
    let mut dep_names = std::collections::HashMap::new();
    for dep in manifest.all_dependencies() {
        if dep.name.is_empty() {
            errors.push("依赖名称不能为空".to_string());
        }
        if dep.version.is_empty() {
            errors.push(format!("依赖 '{}' 缺少版本约束", dep.name));
        }
        let key = format!("{}:{}", dep.name, dep.config);
        if dep_names.insert(key, true).is_some() {
            errors.push(format!("重复依赖: {} ({})", dep.name, dep.config));
        }
    }

    // 7. 自定义任务校验
    let mut task_names = std::collections::HashSet::new();
    for task in &manifest.tasks {
        if task.name.is_empty() {
            errors.push("任务名称不能为空".to_string());
        }
        if !task_names.insert(task.name.clone()) {
            errors.push(format!("重复任务名: {}", task.name));
        }
    }

    // 8. 别名校验
    for (alias, target) in &manifest.build.alias {
        if alias.is_empty() {
            errors.push("别名不能为空".to_string());
        }
        if target.is_empty() {
            errors.push(format!("别名 '{}' 的目标不能为空", alias));
        }
    }

    errors
}

/// 校验包名是否合法
fn is_valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_'
        })
}

/// 校验版本号是否合法（简化 SemVer 检查）
fn is_valid_version(version: &str) -> bool {
    if version.is_empty() {
        return false;
    }
    // 简单的版本格式检查：至少 x.y
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts[0].parse::<u64>().is_ok() && parts[1].parse::<u64>().is_ok()
}
