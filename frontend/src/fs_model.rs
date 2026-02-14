use std::collections::HashMap;
use std::path::PathBuf;
use ignore::{WalkBuilder, gitignore::GitignoreBuilder};

pub struct GitignoreChecker {
    matcher: Option<ignore::gitignore::Gitignore>,
}

impl GitignoreChecker {
    pub fn new(root_path: &PathBuf) -> Self {
        let mut builder = GitignoreBuilder::new(root_path);

        // Add .gitignore if it exists
        let gitignore_path = root_path.join(".gitignore");
        if gitignore_path.exists() {
            builder.add(gitignore_path);
        }

        let matcher = builder.build().ok();

        Self { matcher }
    }

    pub fn is_ignored(&self, path: &PathBuf) -> bool {
        if let Some(ref matcher) = self.matcher {
            matcher.matched(path, path.is_dir()).is_ignore()
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

#[derive(Debug, Default)]
pub struct FileSystemModel {
    pub nodes: Vec<FileNode>,
    pub path_to_index: HashMap<PathBuf, usize>,
    pub root: Option<usize>,
}

impl FileSystemModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_initial(root_path: PathBuf) -> Self {
        let mut model = FileSystemModel::new();

        // Walk the directory tree, respecting .gitignore
        for result in WalkBuilder::new(&root_path)
            .hidden(false)           // Show hidden files/folders (except those in .gitignore)
            .git_ignore(true)        // Respect .gitignore files
            .git_exclude(true)       // Respect .git/info/exclude
            .follow_links(false)     // Don't follow symlinks
            .build()
        {
            if let Ok(entry) = result {
                let path = entry.path().to_path_buf();
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let depth = entry.depth();

                let name = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();

                model.add_node_internal(path, name, is_dir, depth);
            }
        }

        model
    }

    fn add_node_internal(
        &mut self,
        path: PathBuf,
        name: String,
        is_dir: bool,
        depth: usize,
    ) -> usize {
        let index = self.nodes.len();

        // Find parent
        let parent = path.parent().and_then(|p| {
            self.path_to_index.get(p).copied()
        });

        let node = FileNode {
            path: path.clone(),
            name,
            is_dir,
            depth,
            children: Vec::new(),
            parent,
        };

        self.nodes.push(node);
        self.path_to_index.insert(path, index);

        // Update parent's children
        if let Some(parent_idx) = parent {
            self.nodes[parent_idx].children.push(index);
        } else {
            // This is the root
            self.root = Some(index);
        }

        index
    }

    pub fn add_node(&mut self, path: PathBuf, is_dir: bool) -> Option<usize> {
        // Don't add if it already exists
        if self.path_to_index.contains_key(&path) {
            return None;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Calculate depth based on parent
        let depth = if let Some(parent_path) = path.parent() {
            self.path_to_index
                .get(parent_path)
                .map(|&idx| self.nodes[idx].depth + 1)
                .unwrap_or(0)
        } else {
            0
        };

        Some(self.add_node_internal(path, name, is_dir, depth))
    }

    pub fn remove_node(&mut self, path: &PathBuf) -> Option<usize> {
        let index = self.path_to_index.remove(path)?;

        // Remove from parent's children
        if let Some(parent_idx) = self.nodes[index].parent {
            self.nodes[parent_idx].children.retain(|&idx| idx != index);
        }

        // Mark as removed (we keep the slot to maintain indices)
        self.nodes[index].children.clear();

        Some(index)
    }

    pub fn get_node(&self, index: usize) -> Option<&FileNode> {
        self.nodes.get(index)
    }

    pub fn get_node_by_path(&self, path: &PathBuf) -> Option<(usize, &FileNode)> {
        let index = *self.path_to_index.get(path)?;
        Some((index, &self.nodes[index]))
    }

    pub fn total_nodes(&self) -> usize {
        self.nodes.len()
    }
}
