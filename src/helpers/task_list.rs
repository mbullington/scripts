use crate::helpers::{
    git::get_git_root,
    resolve::{find_enclosing_unit, read_scripts},
};

pub fn print_tasks_for_current_unit() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return,
    };
    let git_root = match get_git_root(&cwd) {
        Ok(root) => root,
        Err(_) => cwd.clone(),
    };

    let Ok(unit) = find_enclosing_unit(&cwd, &git_root) else {
        return;
    };
    if let Ok(def) = read_scripts(&unit) {
        println!("\nTasks in {}:", unit.display());
        let mut keys: Vec<_> = def.scripts.keys().collect();
        keys.sort();
        for key in keys {
            println!("  :{key}");
        }
        println!("\nTip: run `scripts run <task>` from this unit.");
    }
}
