use crate::helpers::{git::get_git_root, resolve::read_scripts};

pub fn print_tasks_for_current_unit() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return,
    };
    let git_root = match get_git_root(&cwd) {
        Ok(root) => root,
        Err(_) => cwd.clone(),
    };

    let mut current = cwd.as_path();
    while current.starts_with(&git_root) {
        let scripts_path = current.join("SCRIPTS");
        if scripts_path.exists() {
            if let Ok(def) = read_scripts(current) {
                println!("\nTasks in {}:", current.display());
                let mut keys: Vec<_> = def.scripts.keys().collect();
                keys.sort();
                for key in keys {
                    println!("  :{key}");
                }
                println!("\nTip: run `scripts run <task>` from this unit.");
            }
            break;
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
}
