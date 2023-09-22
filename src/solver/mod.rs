
use std::{sync::mpsc::{Sender, Receiver, channel}, collections::HashMap, io::{self, Write}};

use std::thread;

use crate::util::{Ruleset, DFA, SymbolIdx};

pub use self::events::*;
mod events;

mod bfs;
pub use self::bfs::BFSSolver;

mod hash;
pub use self::hash::HashSolver;

mod subset;
pub use self::subset::SubsetSolver;

mod minkid;
pub use self::minkid::MinkidSolver;

#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant};


#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant};

pub trait Solver where Self : Sized + Clone  + Send + 'static {
    fn get_phases() -> Vec<String>;

    #[cfg(not(target_arch = "wasm32"))]
    fn run_debug(&self,
        sig_k : usize) -> (Receiver<(DFAStructure,SSStructure)>, Receiver<std::time::Duration>, thread::JoinHandle<DFA>) {
            let self_clone = self.clone();
            let (dfa_tx, dfa_rx) = channel();
            let (phase_tx, phase_rx) = channel();
            (dfa_rx, phase_rx, thread::spawn(move || {self_clone.run_internal(sig_k, true, dfa_tx, phase_tx)}))
            
        }
    
    //Changing the function signature based on the architecture is disgusting!
    //But ya know what -- so is the state of Rust WASM, so i'm making do.
    #[cfg(target_arch = "wasm32")]
    fn run_debug(&self,
        sig_k : usize) -> (Receiver<(DFAStructure,SSStructure)>, Receiver<std::time::Duration>) {
            let self_clone = self.clone();
            let (dfa_tx, dfa_rx) = channel();
            let (phase_tx, phase_rx) = channel();
            wasm_bindgen_futures::spawn_local(async move {self_clone.run_internal(sig_k, true, dfa_tx, phase_tx);});
            (dfa_rx, phase_rx)
            
        }
    fn run_internal(self,
                    sig_k : usize, 
                    is_debug : bool,
                    dfa_events : Sender<(DFAStructure,SSStructure)>, 
                    phase_events : Sender<std::time::Duration>) -> DFA;
    fn run(&self, sig_k : usize) -> DFA {
        let (dfa_tx, _dfa_rx) = channel();
        let (phase_tx, _phase_rx) = channel();
        self.clone().run_internal(sig_k, false, dfa_tx,phase_tx)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_with_print(&self, sig_k : usize) -> DFA {
        let (dfa_rx, phase_rx, run_handle) = self.run_debug(sig_k);
        let mut phase_idx;
        let mut phase_lens = vec![];
        let mut iterations = 0;
        let phases = Self::get_phases();
        let mut last_len = 0;
        
        while let Ok((partial_dfa, _sig_sets)) = dfa_rx.recv() {
            let mut update_string = format!("Iteration {} | {} states solved, {} new", iterations,partial_dfa.len() ,partial_dfa.len() - last_len);
            last_len = partial_dfa.len();
            phase_idx = 0;
            print!("{}\r",update_string);
            io::stdout().flush().unwrap();
            while phase_idx < phases.len() {
                //Disconnection is guaranteed here -- should send final DFA then dc on both channels
                match phase_rx.recv() {
                    Ok(time) => {phase_lens.push(time);}
                    _ => {break}
                }
                
                update_string.push_str(&format!(" | {}: {}ms",phases[phase_idx],phase_lens.last().unwrap().as_millis()));
                print!("{}\r",update_string);
                io::stdout().flush().unwrap();
                phase_idx += 1;
            }
            iterations += 1;
            println!("{}",update_string);
        }
        run_handle.join().unwrap()
    }

    fn new(ruleset : Ruleset, goal : DFA) -> Self;
}

pub trait SizedSolver {
    fn get_min_input(&self) -> usize;
    fn get_max_input(&self) -> usize;
    fn single_rule_hash(&self, map : &HashMap<Vec<SymbolIdx>,Vec<Vec<SymbolIdx>>>, start_board : &Vec<SymbolIdx>) -> Vec<Vec<SymbolIdx>> {
        let mut result = vec![];
        for lftmst_idx in 0..start_board.len() {
            for slice_length in self.get_min_input()..core::cmp::min(self.get_max_input(),start_board.len()-lftmst_idx)+1 {
                match map.get(&start_board[lftmst_idx..(lftmst_idx+slice_length)]) {
                    Some(new_swaps) => {
                        let new_board = start_board[0..lftmst_idx].to_vec();

                        for new_swap in new_swaps {
                            let mut newest_board = new_board.clone();
                            newest_board.extend(new_swap);
                            newest_board.extend(start_board[lftmst_idx+slice_length..start_board.len()].to_vec());
                            result.push(newest_board);
                        }
                    }
                    None => {}
                }
            }
        }
        result
    }
    fn sized_init(rules : &Ruleset) -> (usize, usize) {
        let mut min_input : usize = usize::MAX;
        let mut max_input : usize = 0;
        for i in &rules.rules {
            let input_len = i.0.len();
            if input_len < min_input {
                min_input = input_len;
            }
            if input_len > max_input {
                max_input = input_len;
            }
        }
        (min_input, max_input)
    }
}


/*
method to solve a string
todo: implement generically

    fn solve_string(&self, possible_dfa : &DFA, input_str : &Vec<SymbolIdx>) -> Vec<Vec<SymbolIdx>> {
        let mut intrepid_str = input_str.clone();
        let mut visited = HashSet::new();
        let mut result = vec![intrepid_str.clone()];
        visited.insert(intrepid_str.clone());
        while !self.goal.contains(&intrepid_str) {
            for option in self.rules.single_rule_hash(&intrepid_str) {
                if !visited.contains(&option) && possible_dfa.contains(&option) {
                    //println!("{}",symbols_to_string(&intrepid_str));
                    intrepid_str = option;
                    result.push(intrepid_str.clone());
                    visited.insert(intrepid_str.clone());
                }
            }
        }
        //println!("{}",symbols_to_string(&intrepid_str));
        result
    }

*/

/*
old testing methods. they're takin a nap here while I decide what to do with em

fn verify_to_len(&mut self,test_dfa : DFA, n:usize) -> bool{
    //almost certainly a constant time answer to this but idk and idc
    let mut total_boards = 0;
    for i in 0..(n+1) {
        total_boards += (self.symbol_set.length as u64).pow(i as u32);
    }
    
    println!("Starting DFA verification for strings <= {}. {} total boards",n, total_boards);
    let mut num_completed = 0;
    let mut num_accepting = 0;
    let mut start_index = 0;

    let (input, output) = self.create_workers(WORKERS);

    let mut signature_set_old : Vec<Vec<SymbolIdx>> = vec![];
    let mut signature_set_new : Vec<Vec<SymbolIdx>> = vec![vec![]];
    for _ in 0..n {
        std::mem::swap(&mut signature_set_old, &mut signature_set_new);
        signature_set_new.clear();
        for (idx,i) in signature_set_old.iter().enumerate() {
            for symbol in 0..(self.symbol_set.length as SymbolIdx) {
                signature_set_new.push(i.clone());
                signature_set_new.last_mut().unwrap().push(symbol);
                let test_board = signature_set_new.last().unwrap();
                input.push((test_board.clone(),(idx*self.symbol_set.length + (symbol as usize))));
            }
        }
        let mut num_recieved = 0;
        while num_recieved < signature_set_new.len() {
            match output.pop() {
            Some((bfs_result,idx)) => {
                let test_board = &signature_set_new[idx];
                num_completed += 1;
                num_recieved += 1;
                if (num_completed) % (total_boards / 10) == 0 {
                    println!("{}% complete! ({} boards completed)", 100 * num_completed / total_boards, num_completed);
                }
                if bfs_result {num_accepting += 1}
                if test_dfa.contains(&test_board) != bfs_result {
                    println!("Damn. DFA-solvability failed.");
                    println!("Problem board: {}",symbols_to_string(&test_board));
                    println!("DFA: {}, BFS: {}",!bfs_result,bfs_result);
                    return false;
                }
            }
            None => {std::thread::sleep(time::Duration::from_millis(100));}
            }
        }
    }
    self.terminate_workers(input, WORKERS);

        
    println!("All verified! {}% accepting",(num_accepting as f64) * 100.0 / (total_boards as f64));

    true

}
fn random_tests(&mut self,test_dfa : DFA, n:usize, total_boards:usize){
    //almost certainly a constant time answer to this but idk and idc
    
    println!("Starting DFA verification for {} strings of length {}.",total_boards, n);
    let mut num_completed = 0;
    let mut num_accepting = 0;
    let mut start_index = 0;

    let (input, output) = self.create_workers(WORKERS);

    let mut test_items : Vec<Vec<SymbolIdx>> = vec![];
    let mut rng = rand::thread_rng();
    for i in 0..total_boards {
        let mut new_board = vec![];
        for _ in 0..n {
            new_board.push(rng.gen_range(0..self.symbol_set.length) as SymbolIdx);
        }
        input.push((new_board.clone(),i));
        test_items.push(new_board);
    }

    let mut num_recieved = 0;
    while num_recieved < total_boards {
        match output.pop() {
        Some((bfs_result,idx)) => {
            let test_board = &test_items[idx];
            num_completed += 1;
            num_recieved += 1;
            if (num_completed) % (total_boards / 10) == 0 {
                println!("{}% complete! ({} boards completed)", 100 * num_completed / total_boards, num_completed);
            }
            if bfs_result {num_accepting += 1}
            if test_dfa.contains(&test_board) != bfs_result {
                println!("Damn. DFA-solvability failed.");
                println!("Problem board: {}",symbols_to_string(&test_board));
                println!("DFA: {}, BFS: {}",!bfs_result,bfs_result);
                return;
            }
        }
        None => {std::thread::sleep(time::Duration::from_millis(100));}
        }
    }
    self.terminate_workers(input, WORKERS);

        
    println!("All verified! {}% accepting",(num_accepting as f64) * 100.0 / (total_boards as f64));

}
 */