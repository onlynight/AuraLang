//! [Phase B2] 任务图数据结构 + 拓扑排序 + 循环依赖检测
//!
//! 使用 Kahn 算法实现拓扑排序，附带 DFS 循环检测（含环路径报告）。
//!
//! 执行流程：
//! 1. 构建任务图（解析 aura.toml → 展开内置任务 → 建立 depends_on 边）
//! 2. 拓扑排序（检测循环依赖 → 报错）
//! 3. 计算执行层（并行调度用）
//! 4. 增量检查（fingerprint 比对）
//! 5. 执行任务
//! 6. 缓存更新

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::LoomError;
use crate::task::TaskGraph;

/// 拓扑排序结果
#[derive(Debug, Clone, Default)]
pub struct TopoSortResult {
    /// 按拓扑顺序排列的任务名
    pub order: Vec<String>,
    /// 执行层（同一层的任务可并行执行）
    pub layers: Vec<Vec<String>>,
}

impl TopoSortResult {
    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

impl TaskGraph {
    /// 构建邻接表（from → depends_on）
    pub fn build_edges(&self) -> HashMap<String, Vec<String>> {
        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        for task in self.tasks.values() {
            edges.entry(task.name.clone()).or_default();
        }
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                edges.entry(dep.clone()).or_default();
                if let Some(v) = edges.get_mut(&task.name) {
                    if !v.contains(dep) {
                        v.push(dep.clone());
                    }
                }
            }
        }
        edges
    }

    /// 构建入度表（多少任务依赖我）
    pub fn build_in_degree(&self) -> HashMap<String, usize> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        for task in self.tasks.values() {
            in_degree.entry(task.name.clone()).or_insert(0);
        }
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                if in_degree.contains_key(dep) {
                    *in_degree.entry(task.name.clone()).or_insert(0) += 1;
                }
            }
        }
        in_degree
    }

    /// 构建反向邻接表（谁依赖我）
    pub fn build_reverse_edges(&self) -> HashMap<String, Vec<String>> {
        let mut reverse: HashMap<String, Vec<String>> = HashMap::new();
        for task in self.tasks.values() {
            reverse.entry(task.name.clone()).or_default();
        }
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                if self.tasks.contains_key(dep) {
                    let v = reverse.entry(dep.clone()).or_default();
                    if !v.contains(&task.name) {
                        v.push(task.name.clone());
                    }
                }
            }
        }
        reverse
    }

    /// 拓扑排序（Kahn 算法）
    ///
    /// 返回按拓扑顺序排列的任务名和执行层。
    /// 如果存在循环依赖，返回 Err。
    pub fn topological_sort(&self) -> Result<TopoSortResult, LoomError> {
        let in_degree = self.build_in_degree();
        let reverse_edges = self.build_reverse_edges();

        // 第 1 层：所有入度为 0 的任务
        let mut current_layer: Vec<String> = Vec::new();
        let mut processed: HashSet<String> = HashSet::new();

        for (name, &deg) in &in_degree {
            if deg == 0 {
                current_layer.push(name.clone());
                processed.insert(name.clone());
            }
        }

        let mut order: Vec<String> = Vec::new();
        let mut layers: Vec<Vec<String>> = Vec::new();

        // Kahn 算法逐层展开
        while !current_layer.is_empty() {
            layers.push(current_layer.clone());
            for name in &current_layer {
                order.push(name.clone());
            }

            // 计算下一层
            let mut next_layer: Vec<String> = Vec::new();
            let mut next_layer_set: HashSet<String> = HashSet::new();
            for name in &current_layer {
                if let Some(depended_by) = reverse_edges.get(name) {
                    for dep_name in depended_by {
                        if !processed.contains(dep_name) && !next_layer_set.contains(dep_name) {
                            // 检查 dep_name 的所有依赖是否都已完成（只看已完成的层）
                            if let Some(task) = self.tasks.get(dep_name) {
                                let all_deps_done =
                                    task.depends_on.iter().all(|d| processed.contains(d));
                                if all_deps_done {
                                    next_layer.push(dep_name.clone());
                                    next_layer_set.insert(dep_name.clone());
                                }
                            }
                        }
                    }
                }
            }
            // 更新 processed（下一层的任务标记为已处理）
            for name in &next_layer {
                processed.insert(name.clone());
            }
            current_layer = next_layer;
        }

        // 如果排序后的任务数不等于总任务数，说明存在循环
        if order.len() != self.tasks.len() {
            return Err(self.detect_cycle());
        }

        Ok(TopoSortResult {
            order,
            layers,
        })
    }

    /// 循环依赖检测（DFS）
    ///
    /// 返回包含环路径的错误信息。
    pub fn detect_cycle(&self) -> LoomError {
        let edges = self.build_edges();
        let mut visited: HashSet<String> = HashSet::new();
        let mut in_stack: HashSet<String> = HashSet::new();
        let mut stack: Vec<String> = Vec::new();

        for start in self.tasks.keys() {
            if !visited.contains(start) {
                if let Some(cycle) =
                    self.dfs_find_cycle(start, &edges, &mut visited, &mut in_stack, &mut stack)
                {
                    return LoomError::Task(format!(
                        "Cycle detection: task has circular dependency: {}",
                        cycle.join(" → ")
                    ));
                }
            }
        }

        // 不应该到达这里
        LoomError::Task("Unknown error: cycle detection failed".to_string())
    }

    fn dfs_find_cycle(
        &self,
        node: &str,
        edges: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        in_stack.insert(node.to_string());
        stack.push(node.to_string());

        if let Some(deps) = edges.get(node) {
            for dep in deps {
                if !visited.contains(dep) {
                    if let Some(cycle) = self.dfs_find_cycle(dep, edges, visited, in_stack, stack) {
                        return Some(cycle);
                    }
                } else if in_stack.contains(dep) {
                    // 找到循环
                    let cycle_start = stack.iter().position(|s| s == dep).unwrap_or(0);
                    return Some(stack[cycle_start..].to_vec());
                }
            }
        }

        stack.pop();
        in_stack.remove(node);
        None
    }

    /// 查找从某个目标任务可达的所有任务（包含自身）
    ///
    /// 用于构建从 CLI 命令到任务图的映射。
    pub fn find_reachable(&self, root: &str) -> Result<Vec<String>, LoomError> {
        if !self.tasks.contains_key(root) {
            return Err(LoomError::Task(format!("Task '{}' does not exist", root)));
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(root.to_string());
        visited.insert(root.to_string());

        while let Some(name) = queue.pop_front() {
            if let Some(task) = self.tasks.get(&name) {
                for dep in &task.depends_on {
                    if !visited.contains(dep) {
                        if !self.tasks.contains_key(dep) {
                            return Err(LoomError::Task(format!(
                                "Task '{}' depends on non-existent task '{}'",
                                name, dep
                            )));
                        }
                        visited.insert(dep.clone());
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        Ok(visited.into_iter().collect())
    }

    /// 计算任务层（用于并行调度）
    pub fn compute_layers(&self) -> Result<Vec<Vec<String>>, LoomError> {
        let result = self.topological_sort()?;
        Ok(result.layers)
    }

    /// 检查任务图是否有效（无缺失依赖、无循环）
    pub fn validate(&self) -> Result<(), LoomError> {
        // 1. 检查依赖是否都存在
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                if !self.tasks.contains_key(dep) {
                    return Err(LoomError::Task(format!(
                        "Task '{}' depends on non-existent task '{}'",
                        task.name, dep
                    )));
                }
            }
        }

        // 2. 检查循环依赖
        let edges = self.build_edges();
        let mut visited: HashSet<String> = HashSet::new();
        let mut in_stack: HashSet<String> = HashSet::new();
        let mut stack: Vec<String> = Vec::new();

        for start in self.tasks.keys() {
            if !visited.contains(start) {
                if let Some(cycle) =
                    self.dfs_find_cycle(start, &edges, &mut visited, &mut in_stack, &mut stack)
                {
                    return Err(LoomError::Task(format!(
                        "Cycle detection: task has circular dependency: {}",
                        cycle.join(" → ")
                    )));
                }
            }
        }

        Ok(())
    }

    /// 获取任务及其所有传递依赖（包含自身）
    pub fn with_all_dependencies(&self, name: &str) -> Result<Vec<String>, LoomError> {
        self.find_reachable(name)
    }

    /// 移除任务（同时清理所有引用该任务的依赖关系）
    pub fn remove_task(&mut self, name: &str) {
        self.tasks.remove(name);
        for task in self.tasks.values_mut() {
            task.depends_on.retain(|dep| dep != name);
        }
    }

    /// 获取所有任务名
    pub fn task_names(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }

    /// 获取依赖指定任务的所有任务
    pub fn dependents_of(&self, name: &str) -> Vec<String> {
        self.build_reverse_edges().get(name).cloned().unwrap_or_default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};

    fn make_task(name: &str, deps: &[&str]) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: format!("test task {}", name),
            kind: TaskKind::Clean,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        }
    }

    #[test]
    fn test_build_edges_simple() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["b"]));

        let edges = graph.build_edges();
        assert_eq!(edges.get("a").unwrap().len(), 0);
        assert_eq!(edges.get("b").unwrap(), &vec!["a".to_string()]);
        assert_eq!(edges.get("c").unwrap(), &vec!["b".to_string()]);
    }

    #[test]
    fn test_topological_sort_linear() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["b"]));

        let result = graph.topological_sort().unwrap();
        let a_pos = result.order.iter().position(|n| n == "a").unwrap();
        let b_pos = result.order.iter().position(|n| n == "b").unwrap();
        let c_pos = result.order.iter().position(|n| n == "c").unwrap();
        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
        assert_eq!(result.order.len(), 3);
    }

    #[test]
    fn test_topological_sort_diamond() {
        // Diamond: a → b, a → c, b → d, c → d
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["a"]));
        graph.add_task(make_task("d", &["b", "c"]));

        let result = graph.topological_sort().unwrap();
        let a_pos = result.order.iter().position(|n| n == "a").unwrap();
        let b_pos = result.order.iter().position(|n| n == "b").unwrap();
        let c_pos = result.order.iter().position(|n| n == "c").unwrap();
        let d_pos = result.order.iter().position(|n| n == "d").unwrap();
        assert!(a_pos < b_pos);
        assert!(a_pos < c_pos);
        assert!(b_pos < d_pos);
        assert!(c_pos < d_pos);
    }

    #[test]
    fn test_topological_sort_parallel_layers() {
        // a and b are independent, c depends on both
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &[]));
        graph.add_task(make_task("c", &["a", "b"]));

        let result = graph.topological_sort().unwrap();
        // Layer 0: a, b
        // Layer 1: c
        assert_eq!(result.layers.len(), 2);
        let layer0: Vec<_> = result.layers[0].iter().collect();
        assert!(layer0.contains(&&"a".to_string()));
        assert!(layer0.contains(&&"b".to_string()));
        assert_eq!(result.layers[1].len(), 1);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &["c"]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["b"]));

        let result = graph.topological_sort();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.to_lowercase().contains("circular dependency"));
    }

    #[test]
    fn test_validate_missing_dep() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &["nonexistent"]));

        let result = graph.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.to_lowercase().contains("non-existent task"));
    }

    #[test]
    fn test_find_reachable() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        graph.add_task(make_task("resolve", &["clean"]));
        graph.add_task(make_task("compile", &["resolve"]));
        graph.add_task(make_task("test", &["compile"]));
        graph.add_task(make_task("package", &["compile"]));

        let reachable = graph.find_reachable("test").unwrap();
        assert!(reachable.contains(&"test".to_string()));
        assert!(reachable.contains(&"compile".to_string()));
        assert!(reachable.contains(&"resolve".to_string()));
        assert!(reachable.contains(&"clean".to_string()));
        assert!(!reachable.contains(&"package".to_string()));
    }

    #[test]
    fn test_find_reachable_nonexistent() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));

        let result = graph.find_reachable("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_task() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["b"]));

        graph.remove_task("b");
        assert!(graph.get("b").is_none());
        let c = graph.get("c").unwrap();
        assert!(c.depends_on.is_empty());
    }

    #[test]
    fn test_task_names() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &[]));
        graph.add_task(make_task("c", &[]));

        let names = graph.task_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert!(names.contains(&"c".to_string()));
    }

    #[test]
    fn test_empty_graph() {
        let graph = TaskGraph::new();
        let result = graph.topological_sort().unwrap();
        assert!(result.is_empty());
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn test_standard_build_lifecycle() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        graph.add_task(make_task("resolve", &["clean"]));
        graph.add_task(make_task("compile-main", &["resolve"]));
        graph.add_task(make_task("compile-test", &["compile-main"]));
        graph.add_task(make_task("run-tests", &["compile-test"]));
        graph.add_task(make_task("package", &["compile-main"]));
        graph.add_task(make_task("verify", &["package"]));
        graph.add_task(make_task("install", &["verify"]));

        let result = graph.topological_sort().unwrap();
        assert_eq!(result.order.len(), 8);
        assert!(result.layers.len() >= 5);
        assert_eq!(result.order.first().unwrap(), "clean");
        assert_eq!(result.order.last().unwrap(), "install");
    }

    #[test]
    fn test_compute_layers() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &[]));
        graph.add_task(make_task("c", &["a"]));
        graph.add_task(make_task("d", &["b", "c"]));

        let layers = graph.compute_layers().unwrap();
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].len(), 2);
        assert_eq!(layers[1], vec!["c".to_string()]);
        assert_eq!(layers[2], vec!["d".to_string()]);
    }

    #[test]
    fn test_self_cycle_detection() {
        // Self-loop: a depends on a
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &["a"]));

        let result = graph.topological_sort();
        assert!(result.is_err());
    }

    #[test]
    fn test_depended_on_by() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        graph.add_task(make_task("c", &["a"]));

        let deps = graph.dependents_of("a");
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"b".to_string()));
        assert!(deps.contains(&"c".to_string()));
    }

    #[test]
    fn test_validate_ok() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));

        assert!(graph.validate().is_ok());
    }

    #[test]
    fn test_multiple_roots() {
        // Multiple independent roots
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        graph.add_task(make_task("fmt", &[]));
        graph.add_task(make_task("compile", &["clean"]));
        graph.add_task(make_task(
            "package",
            &[
                "compile", "fmt",
            ],
        ));

        let result = graph.topological_sort().unwrap();
        // clean and fmt should be in layer 0
        assert!(result.layers[0].contains(&"clean".to_string()));
        assert!(result.layers[0].contains(&"fmt".to_string()));
        assert_eq!(result.layers.last().unwrap(), &vec!["package".to_string()]);
    }
}
