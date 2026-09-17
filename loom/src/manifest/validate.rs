//! 配置校验
//!
//! [Phase L1/B1.5] schema 校验 + 错误报告

use crate::manifest::LoomManifest;

/// 校验 Manifest，返回错误列表（空 = 通过）
pub fn validate_manifest(manifest: &LoomManifest) -> Vec<String> {
    let mut errors = Vec::new();

    // 1. 包名校验
    if manifest.name.is_empty() {
        errors.push("Required field missing: name".to_string());
    } else if !is_valid_package_name(&manifest.name) {
        errors.push(format!(
            "Invalid package name: '{}' (only lowercase letters, digits, hyphens, and dots allowed)",
            manifest.name
        ));
    }

    // 2. 版本号校验
    if manifest.version.is_empty() {
        errors.push("Required field missing: version".to_string());
    } else if !is_valid_version(&manifest.version) {
        errors.push(format!(
            "Invalid version number: '{}' (must conform to SemVer format)",
            manifest.version
        ));
    }

    // 3. 入口文件校验（仅应用包需要）
    if !manifest.library && manifest.entry.is_empty() {
        errors.push("Application packages must specify an entry file".to_string());
    }

    // 4. 构建配置校验
    if let Some(ref ws) = manifest.workspace {
        // Workspace 根配置不应有 entry
        if !manifest.entry.is_empty() {
            errors.push("Workspace root config should not specify entry (entry should be in member project config)".to_string());
        }
        if ws.members.is_empty() {
            errors.push("[workspace] must specify at least one member".to_string());
        }
        for member in &ws.members {
            if member.is_empty() {
                errors.push("[workspace] members contains empty path".to_string());
            }
        }
    }

    // 5. Profile 校验
    let active_profiles: Vec<_> = manifest.profiles.iter().filter(|(_, p)| p.activate).collect();
    if active_profiles.len() > 1 {
        errors.push(format!(
            "Only one profile can be active at most, currently active: {}",
            active_profiles.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    // 6. 依赖校验
    let mut dep_names = std::collections::HashMap::new();
    for dep in manifest.all_dependencies() {
        if dep.name.is_empty() {
            errors.push("Dependency name cannot be empty".to_string());
        }
        if dep.version.is_empty() {
            errors.push(format!(
                "Dependency '{}' is missing version constraint",
                dep.name
            ));
        }
        let key = format!("{}:{}", dep.name, dep.config);
        if dep_names.insert(key, true).is_some() {
            errors.push(format!(
                "Duplicate dependency: {} ({})",
                dep.name, dep.config
            ));
        }
    }

    // 7. 自定义任务校验
    let mut task_names = std::collections::HashSet::new();
    for task in &manifest.tasks {
        if task.name.is_empty() {
            errors.push("Task name cannot be empty".to_string());
        }
        if !task_names.insert(task.name.clone()) {
            errors.push(format!("Duplicate task name: {}", task.name));
        }
    }

    // 8. 别名校验
    for (alias, target) in &manifest.build.alias {
        if alias.is_empty() {
            errors.push("Alias cannot be empty".to_string());
        }
        if target.is_empty() {
            errors.push(format!("Alias '{}' target cannot be empty", alias));
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
