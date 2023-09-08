use std::hash::Hash;
use bitvec::prelude::*;
use petgraph::algo::toposort;

use petgraph::prelude::*;
use crossbeam::queue::SegQueue;
use petgraph::graph::{NodeIndex, DiGraph};
use petgraph::dot::Dot;

use std::collections::{HashMap,HashSet};
use petgraph::Graph;
use std::fs;
use std::time;
use std::sync::Arc;
extern crate xml;
use std::fs::File;
use std::io::{self, Write};
use std::fmt::Debug;
use rand::prelude::*;

use petgraph::algo::condensation;

use serde_json::Result;

use xml::writer::{EmitterConfig, XmlEvent};
use std::time::Instant;


use serde::{Deserialize, Serialize};
use std::thread;

const WORKERS : usize = 37;

type SymbolIdx = u8;


#[derive(Clone,Serialize,Deserialize,Debug)]
struct SymbolSet {
    length : usize,
    representations : Vec<String>
}
impl SymbolSet {
    fn new(representations : Vec<String>) -> SymbolSet{
        SymbolSet { length: representations.len(), representations: representations }
    }

    fn find_in_sig_set<'a>(&self, string : impl Iterator<Item = &'a SymbolIdx>) -> usize
    {
        let mut result = 0;
        for sym in string {
            result *= self.length;
            result += *sym as usize + 1;
        }
        result
    }
    fn idx_to_element(&self, mut idx : usize) -> Vec<SymbolIdx>
    {
        let mut result = vec![];
        while idx > 0 {
            idx -= 1;
            result.push((idx % self.length) as SymbolIdx);
            idx /= self.length;
        }
        result.reverse();
        result
    }
}

#[derive(Clone)]
struct Ruleset  {
    min_input : usize,
    max_input : usize,
    rules : Vec<(Vec<SymbolIdx>,Vec<SymbolIdx>)>,
    symbol_set : SymbolSet,
    map : HashMap<Vec<SymbolIdx>, Vec<Vec<SymbolIdx>>>, //need this for speed : )
    reverse_map : HashMap<Vec<SymbolIdx>, Vec<Vec<SymbolIdx>>>
}

impl Ruleset {
    fn new(rules : Vec<(Vec<SymbolIdx>,Vec<SymbolIdx>)>, symbol_set : SymbolSet) -> Ruleset{
        let mut min_input : usize = usize::MAX;
        let mut max_input : usize = 0;
        let mut rule_hash : HashMap<Vec<SymbolIdx>,Vec<Vec<SymbolIdx>>> = HashMap::new();
        let mut reverse_rule_hash : HashMap<Vec<SymbolIdx>,Vec<Vec<SymbolIdx>>> = HashMap::new();
        //Should use a fancy map function here I admit
        for i in &rules {
            let input_len = i.0.len();
            if input_len < min_input {
                min_input = input_len;
            }
            if input_len > max_input {
                max_input = input_len;
            }
            match rule_hash.get_mut(&i.0) {
                Some(result_vec) => {result_vec.push(i.1.clone())},
                None => {rule_hash.insert(i.0.clone(), vec![i.1.clone()]);}
            }
            match reverse_rule_hash.get_mut(&i.1) {
                Some(result_vec) => {result_vec.push(i.0.clone())},
                None => {reverse_rule_hash.insert(i.1.clone(), vec![i.0.clone()]);}
            }
        }
        
        Ruleset { min_input: min_input, 
                max_input: max_input, 
                rules: rules, 
                symbol_set: symbol_set, 
                map: rule_hash,
                reverse_map : reverse_rule_hash }
    }
    
    fn rule_applications(&self, start_board : &Vec<SymbolIdx>) -> Vec<(usize, usize)>{
        let mut index = 0;
        let mut result = vec![];
        while index < start_board.len(){
            for (rule_idx,rule) in self.rules.iter().enumerate() {
                let end_index = index+rule.0.len();
                if end_index <= start_board.len() && rule.0[..] == start_board[index..end_index] {
                    result.push((rule_idx,index))                    
                }
            }
            index += 1;
        }
        result
    }
    fn apply_rule(&self, start_board : &Vec<SymbolIdx>, rule_idx : usize, rule_pos : usize) -> Vec<SymbolIdx> {
        let rule = &self.rules[rule_idx];
        let mut new_board = start_board[0..rule_pos].to_vec();
        new_board.extend(rule.1.clone());
        new_board.extend(start_board[rule_pos+rule.0.len()..start_board.len()].to_vec());
        /* 
        for new_sym in &rule.1 {
            
            new_board[copy_index] = *new_sym;
            copy_index += 1;
        }*/
        new_board
    }
    fn single_rule(&self, start_board : &Vec<SymbolIdx>) -> Vec<Vec<SymbolIdx>> {
        let mut result = vec![];
        for new_option in self.rule_applications(start_board) {
            result.push(self.apply_rule(&start_board,new_option.0,new_option.1));
        }
        result
    }
    //Do we need to do this? honestly i don't think so.
    //Feels like we'd benefit from much faster (manual) 2d operations
    //i.e. keeping the same amounts of rules as 1d but crawling through rows + columns (basically no diagonal moves allowed lol).
    //or maybe there's a way to keep diagonal moves too idk
    fn single_rule_hash(&self, start_board : &Vec<SymbolIdx>) -> Vec<Vec<SymbolIdx>> {
        let mut result = vec![];
        for lftmst_idx in 0..start_board.len() {
            for slice_length in (self.min_input..core::cmp::min(self.max_input,start_board.len()-lftmst_idx)+1) {
                match self.map.get(&start_board[lftmst_idx..(lftmst_idx+slice_length)]) {
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
    fn reverse_single_rule_hash(&self, start_board : &Vec<SymbolIdx>) -> Vec<Vec<SymbolIdx>> {
        let mut result = vec![];
        for lftmst_idx in 0..start_board.len() {
            for slice_length in (self.min_input..core::cmp::min(self.max_input,start_board.len()-lftmst_idx)+1) {
                match self.reverse_map.get(&start_board[lftmst_idx..(lftmst_idx+slice_length)]) {
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

    fn reverse_single_rule_hash_fucko(&self, start_board : &Vec<SymbolIdx>, immutably_threshold : usize) -> Vec<Vec<SymbolIdx>> {
        let mut result = vec![];
        for lftmst_idx in immutably_threshold..start_board.len() {
            for slice_length in (self.min_input..core::cmp::min(self.max_input,start_board.len()-lftmst_idx)+1) {
                match self.reverse_map.get(&start_board[lftmst_idx..(lftmst_idx+slice_length)]) {
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

    fn all_reverse_from(&self,start_boards : &Vec<Vec<SymbolIdx>>, result_map : &mut HashSet<Vec<SymbolIdx>> ) {
        let mut old_boards = vec![];
        let mut new_boards = start_boards.clone();
        while new_boards.len() > 0 {
            std::mem::swap(&mut old_boards, &mut new_boards);
            new_boards.clear();
            for old_board in &old_boards {
                for potential_board in self.reverse_single_rule_hash(old_board) {
                    if result_map.insert(potential_board.clone()) {
                        new_boards.push(potential_board);
                    }
                }
            }
        }
        
    }
    fn all_reverse_from_fucko(&self,start_boards : &Vec<Vec<SymbolIdx>>, immutably_threshold : usize) -> HashSet<Vec<SymbolIdx>> {
        let mut result_map : HashSet<Vec<SymbolIdx>> = HashSet::new();
        let mut old_boards = vec![];
        let mut new_boards = start_boards.clone();
        while new_boards.len() > 0 {
            std::mem::swap(&mut old_boards, &mut new_boards);
            new_boards.clear();
            for old_board in &old_boards {
                for potential_board in self.reverse_single_rule_hash(old_board) {
                    if !result_map.contains(&potential_board) {
                        new_boards.push(potential_board.clone());
                        result_map.insert(potential_board);
                    }
                }
            }
        }
        result_map
    }
}

fn worker_thread(translator : Arc<SRSTranslator>, input : Arc<SegQueue<(Vec<SymbolIdx>,usize)>>, output : Arc<SegQueue<(bool,usize)>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while true {
            match input.pop() {
                Some(input_string) => 
                {
                    if input_string.1 == usize::MAX && input_string.0 == vec![69,42] {
                        return;
                    }
                    let result = translator.bfs_solver_batch(&input_string.0);
                    output.push((result,input_string.1));
                }
                None => {}//{std::thread::sleep(time::Duration::from_millis(10));}
            }
        }
    })
}

#[derive(Clone,Serialize,Deserialize)]
struct DFA {
    starting_state : usize,
    state_transitions : Vec<Vec<usize>>,
    accepting_states : HashSet::<usize>,
    symbol_set : SymbolSet
}

#[derive(Clone)]
struct SRSTranslator {
    rules : Ruleset,
    goal : DFA,
    board_solutions : HashMap<Vec<SymbolIdx>,bool>,
    symbol_set : SymbolSet,
    //signature sets of all known, then prospective states
    sig_sets : Vec<BitVec>,

    //which elements of the signature set are solved; for each prospective state
    solved_yet : Vec<BitVec>,

    //HashMap of known states' signature sets, used for uniqueness test
    unique_sigs : HashMap<BitVec,usize>,

    //2-D transition table of all known, the prospective states. 
    trans_table : Vec<Vec<usize>>,

    //Link graph of signature set elements
    ss_link_graph : DiGraph<SignatureSetElement,()>,

    //For each state of the goal DFA, what would its hypothetical minkid set look like?
    //Used as the basis for propagation in the minkid method
    goal_minkids : Vec<HashSet<NodeIndex>>,

    //Lookup table of where individual ss elements ended up in the graph
    ss_idx_to_link : Vec<NodeIndex>
}

#[derive(Debug,Clone,Hash)]
struct DFAState {
    solved_children : usize,
    sig_set : BitVec,
    trans_states : Vec<usize>
}

#[derive(Debug,Clone,Hash)]
struct ProspectiveDFAState {
    solved_parents : usize,
    sig_set : BitVec,
    known_answer : BitVec,
    origin : usize,
    origin_char : SymbolIdx
}

struct MKDFAState {
    minkids : HashSet<NodeIndex>,
    goal_states : Vec<usize>
}

#[derive(Debug,Clone, Default)]
struct SignatureSetElement {
    //Original elements of the signature set that this one node now represents
    original_idxs : Vec<usize>,
    //Pre-computed set of ancestors -- used under the assumption that pre-calculating this will ultimately make things way faster
    //assumption is wrong -- memory complexity is ridiculous lol
    //precomputed_ancestors : HashSet<NodeIndex>,
    //DFA states that lead to an accepting string after walking through !!any!! of the original elements for this node
    //Deprecated in favor of goal_minkids in SRS translator
    //accepting_states : Vec<usize>
}


impl SRSTranslator {

    fn new(rules : Ruleset, goal : DFA) -> SRSTranslator {
        let sym_set = goal.symbol_set.clone();
        SRSTranslator { rules: rules, 
            goal: goal, 
            board_solutions: HashMap::new(),
            symbol_set: sym_set,
            sig_sets : vec![],
            solved_yet : vec![],
            unique_sigs : HashMap::new(),
            trans_table : vec![],
            //link_graph for all members of signature set
            ss_link_graph : DiGraph::<SignatureSetElement,()>::new(),
            goal_minkids : vec![],
            ss_idx_to_link : vec![]
         }
    }

    fn minkids_to_tt(&self, sig_set : &Vec<Vec<SymbolIdx>>, minkids : &HashSet<NodeIndex>) -> BitVec {
        let mut result = bitvec![0;sig_set.len()];
        let reversed_graph = petgraph::visit::Reversed(&self.ss_link_graph);
        let mut dfs = Dfs::empty(&reversed_graph);
        for minkid in minkids {
            dfs.move_to(*minkid);
            while let Some(nx) = dfs.next(&reversed_graph) {
                for ss_idx in &self.ss_link_graph[nx].original_idxs {
                    result.set(*ss_idx,true);
                }
            }
        }
        result
    }

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

    fn build_ss_link_graph(&mut self, k : usize, sig_set : &Vec<Vec<SymbolIdx>>){
        let mut ss_link_graph = DiGraph::<usize,()>::with_capacity(sig_set.len(),10);
        //irritated that there is not an immediately obvious better way but w/e
        
        //build initial link graph
        for i in 0..sig_set.len() {
            ss_link_graph.add_node(i);
        }
        for i in 0..sig_set.len() {
            for result in self.rules.single_rule_hash(&sig_set[i]) {
                ss_link_graph.add_edge(NodeIndex::new(i), NodeIndex::new(self.symbol_set.find_in_sig_set(result.iter())), ());
            }
        }
        //Get rid of strongly-connected components
        let ss_link_graph = condensation(ss_link_graph, true);

        self.ss_idx_to_link = vec![NodeIndex::new(0);sig_set.len()];

        //Convert into actually-used data structure
        self.ss_link_graph = DiGraph::new();
        for i in ss_link_graph.node_indices() {
            let mut idxs_clone = ss_link_graph[i].clone();
            idxs_clone.shrink_to_fit();
            for idx in &idxs_clone {
                self.ss_idx_to_link[*idx] = i;
            }
            self.ss_link_graph.add_node(SignatureSetElement { original_idxs: idxs_clone });
        }
        //I would love to care about this. will not yet!
        //self.ss_link_graph.extend_with_edges(self.ss_link_graph.raw_edges().iter());
        for i in ss_link_graph.raw_edges(){
            self.ss_link_graph.add_edge(i.source(), i.target(), ());
        }

        //time to pre-compute ancestors & calculate valid DFA states
        let mut reversed_graph = self.ss_link_graph.clone();
        reversed_graph.reverse();
        
        //Building minkids for each state in the goal DFA
        //Done by performing DFS
        self.goal_minkids = vec![HashSet::new();self.goal.state_transitions.len()];
        //There is a fancier DFS-based way to do this. Do I look like the type to care?
        //(jk again just not pre-emptively optimizing)
        for goal_state in 0..self.goal_minkids.len() {
            //Toposort used so no childer checks needed
            for element in toposort(&reversed_graph, None).unwrap() {
                //Are any of the strings represented by this node accepting?
                let is_accepted = self.ss_link_graph[element].original_idxs.iter().any(|x| self.goal.contains_from_start(&sig_set[*x], goal_state));
                //If it's an accepting state that is not the ancestor of any of the current minkids
                if is_accepted && !self.check_if_ancestor(&self.goal_minkids[goal_state], element) {
                    self.goal_minkids[goal_state].insert(element);
                }
            }
        }
        let mut ss_debug_graph : DiGraph<String,()> = Graph::new();
        for node_idx in self.ss_link_graph.node_indices() {
            let node = &self.ss_link_graph[node_idx];
            let mut final_str = format!("{}:",node_idx.index());
            for i in &node.original_idxs {
                final_str.push_str(&symbols_to_string(&sig_set[*i]));
            }
            ss_debug_graph.add_node(final_str);
        }
        for edge in self.ss_link_graph.raw_edges() {
            ss_debug_graph.add_edge(edge.source(), edge.target(), ());
        }
        let mut file = File::create("link_graph_debug/ss.dot").unwrap();
        file.write_fmt(format_args!("{:?}",Dot::new(&ss_debug_graph))).unwrap();
        /* 
        for i in self.ss_link_graph.node_indices() {
            //Calculating all ancestors
            //Notably, this includes itself. Burns some memory, but allows us to skip what would otherwise be an additional check
           // let mut dfs = Dfs::new(&reversed_graph,i);
            //while let Some(nx) = dfs.next(&reversed_graph) {
            //    self.ss_link_graph[i].precomputed_ancestors.insert(nx);
            //}
            //Calculating valid DFA states
            //old method for building accpeting states for each string -- disliked bc worse for both time/memory complexity
            
            for start in 0..self.goal.state_transitions.len() {
                for element in &self.ss_link_graph[i].original_idxs {
                    if self.goal.contains_from_start(&sig_set[*element], start) {
                        self.ss_link_graph[i].accepting_states.push(start);
                        break
                    }
                }
                self.ss_link_graph[i].accepting_states.shrink_to_fit();
            }
        }*/

        

    }

    //Checks to see if a potentially new element of the minkid set is actually an ancestor to a pre-existing minkid
    //false means it is distinct from the current set
    fn check_if_ancestor(&self, min_children : &HashSet<NodeIndex>, potential : NodeIndex) -> bool {
        //This checks all children of the potential element.
        //If there's a minkid in the children of this potential element, we know that the potential element is redundant
        let mut dfs = Dfs::new(&self.ss_link_graph, potential);
        while let Some(nx) = dfs.next(&self.ss_link_graph) {
            
            if min_children.contains(&nx) {
                return true;
            }
        }
        false
    }
    //checks which elements of the minkid vec are ancestors of a potential minkid element
    //This is currently sub-optimal -- assuming checks are done properly, there are no children of a minkid element that are also within the minkid set
    //this means the DFS checks unnecesary values. But! This is just a sanitation method anyway -- hopefully it's not in the final cut
    fn check_if_childer(&self, min_children : &HashSet<NodeIndex>, potential : NodeIndex) -> HashSet<NodeIndex> {
        let mut result = HashSet::new();
        let reversed_graph = petgraph::visit::Reversed(&self.ss_link_graph);
        let mut dfs = Dfs::new(&reversed_graph, potential);
        while let Some(nx) = dfs.next(&reversed_graph) {
            //If a minkid element is an ancestor to the potential guy
            if min_children.contains(&nx) {
                result.insert(nx);
            }
        }
        result
    }
    //notably sub-optimal -- i am keeping things readble first because I am gonna go cross-eyed if I pre-emptively optimize THIS
    //Returns true if minkids is modified
    fn add_to_minkids(&self, min_children : &mut HashSet<NodeIndex>, potential : NodeIndex) -> bool {
        if self.check_if_ancestor(min_children, potential) {
            return false;
        }
        let redundant_kids = self.check_if_childer(min_children, potential);
        min_children.insert(potential);
        //This could be dumb!
        *min_children = min_children.difference(&redundant_kids).map(|x| *x).collect::<HashSet<_>>();
        return !redundant_kids.is_empty();
    }
    //This could probably be a lot faster... oh well!
    fn add_set_to_minkids(&self, min_children : &mut HashSet<NodeIndex>, potential_kids : &HashSet<NodeIndex>) -> bool {
        let mut modified = false;
        for potential in potential_kids {
            if self.check_if_ancestor(min_children, *potential) {
                continue;
            }
            let redundant_kids = self.check_if_childer(min_children, *potential);
            /* 
            if !redundant_kids.is_empty() {
                println!("Childer is actually useful!");
            }*/
            min_children.insert(*potential);
            //This could be dumb!
            *min_children = min_children.difference(&redundant_kids).map(|x| *x).collect::<HashSet<_>>();
            modified = true;
        }
        modified
    }
    //Call to apply a partial link between two nodes
    fn partial_link(&mut self, dfa_graph : &mut DiGraph<MKDFAState,SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>, connection : &(Vec<SymbolIdx>,Vec<SymbolIdx>), lhs : NodeIndex, rhs : NodeIndex) -> bool{
        //To do this, we need to build an intermediary set of potential minkids that could be provided
        let reversed_graph = petgraph::visit::Reversed(&self.ss_link_graph);
        let mut dfs = Dfs::empty(&reversed_graph);

        let mut intermediary_minkids = HashSet::new();
        //For each bottom minkid,
        for rhs_minkid in &dfa_graph[rhs].minkids {
            //Build a list of minkids that are ancestors of the rhs_minkid and possess a ss element that complies with the obligation
            dfs.move_to(*rhs_minkid);
            while let Some(nx) = dfs.next(&reversed_graph) {
                for ss_idx in &self.ss_link_graph[nx].original_idxs {
                    //If the ss element is actually big enough to comply with the obligation, and does
                    if sig_set[*ss_idx].len() >= connection.1.len() && sig_set[*ss_idx][0..connection.1.len()] == connection.1 {
                        //Build what the new element would look like
                        let mut new_ss = connection.0.clone();
                        new_ss.extend(&sig_set[*ss_idx][connection.0.len()..]);

                        //Find its index, translate it to a node, and add that node to our list of intermediary minkids
                        intermediary_minkids.insert(self.ss_idx_to_link[self.symbol_set.find_in_sig_set(new_ss.iter())]);
                        //Prevent looking further into this area's ancestors
                        //dfs adds all of the unvisited children of the thing to the stack. this stops that
                        /* 
                        for i in reversed_graph.neighbors(nx).collect::<Vec<_>>().iter().rev() {
                            if dfs.discovered.visit(*i) {
                                dfs.stack.pop();
                            }
                        }*/
                        
                        break
                    }
                }
            }
        }
        self.add_set_to_minkids(&mut dfa_graph[lhs].minkids, &intermediary_minkids)
    }

    fn add_link(&self, link_graph : &mut DiGraph<(),(Vec<SymbolIdx>,Vec<SymbolIdx>)>, lhs : NodeIndex, rhs : NodeIndex, lhs_obligation : &[SymbolIdx], rhs_obligation : &[SymbolIdx]) {
        let mut death_row = vec![];
        let mut should_add = true;

        //checking every pre-existing edge to see if
        //1. we make any of them redundant by offering a more flexible alternative
        //2. any of them make our potential link redundant by already being more flexible
        for edge in link_graph.edges_connecting(lhs, rhs) {
            //redundancy check!
            //This currently isn't ~designed~ around anything other than length-preserving strings, bc they stilll confuse me

            let lhs_min = std::cmp::min(lhs_obligation.len(), edge.weight().0.len());
            let rhs_min = std::cmp::min(rhs_obligation.len(), edge.weight().1.len());
            //Check our proposed obligation against the current one. If ours is shorter, compare the prefixes of both such that they maintain equal length
            if &edge.weight().0[..lhs_min] == lhs_obligation && &edge.weight().1[..rhs_min] == rhs_obligation {
                //and the current edge has a greater obligation
                if edge.weight().0.len() > lhs_obligation.len() {
                    death_row.push(edge.id());
                //otherwise, there's no benefit to adding this edge!
                }else {
                    should_add = false;
                    break
                }
            }
        }
        if should_add {
            //This has made me realize these could definitely just be references... but whatever!
            link_graph.add_edge(lhs, rhs, (lhs_obligation.to_vec(),rhs_obligation.to_vec()));
        }
        for dead_edge in &death_row {
            link_graph.remove_edge(*dead_edge);
        }
    }

    fn build_sig_k(&self, k : usize) -> Vec<Vec<SymbolIdx>> {
        //let start_sig_len : usize = (cardinality::<S>() << k)-1;
        let mut start_index = 0;
        let mut signature_set : Vec<Vec<SymbolIdx>> = vec![vec![]];
        let mut end_index = 1;
        let mut new_index = 1;
        for _ in 0..k {
            for i in start_index..end_index{
                for symbol in 0..(self.symbol_set.length as SymbolIdx) {
                    signature_set.push(signature_set[i].clone());
                    signature_set[new_index].push(symbol);
                    new_index += 1;
                }
            }
            start_index = end_index;
            end_index = new_index;
        }
        signature_set
    }

    
    //Way less memory usage because no addition/checking HashMap.
    //Also paralellizable, hence "batch"
    fn bfs_solver_batch(&self, start_board : &Vec<SymbolIdx>) -> bool { 
        let mut new_boards : Vec<Vec<SymbolIdx>> = vec![start_board.clone()];
        let mut old_boards : Vec<Vec<SymbolIdx>> = vec![];
        let mut known_states = HashSet::<Vec<SymbolIdx>>::new();
        known_states.insert(start_board.clone());
        while new_boards.len() > 0 {
            std::mem::swap(&mut old_boards, &mut new_boards);
            new_boards.clear();
            for board in &old_boards {
                if self.goal.contains(board) {
                    return true;
                }
                for new_board in self.rules.single_rule_hash(board) {
                    if !known_states.contains(&new_board) {
                        known_states.insert(new_board.clone());
                        new_boards.push(new_board);
                    }
                }
            }
        }
        false
    }

    fn bfs_solver_sub(&mut self, start_board : &Vec<SymbolIdx>, state_idx : usize, sig_idx : usize, investigated : &mut HashSet<(usize,usize)>) -> bool { 
        /*if !investigated.insert((state_idx,sig_idx)) {
            return false;
        }*/

        if state_idx < self.trans_table.len() || self.solved_yet[state_idx - self.trans_table.len()][sig_idx] {
            return self.sig_sets[state_idx][sig_idx];
        }
        if self.goal.contains(&start_board) {
            self.solved_yet[state_idx - self.trans_table.len()].set(sig_idx,true);
            self.sig_sets[state_idx].set(sig_idx,true);

            //RECURSIVELY INFORM PARENTS THIS SHIT IS TRUE
            //not yet tho : )
            return true;
        }
        //Do not need to update this node if true because recursive thing above should cover it.
        for new_board in self.rules.single_rule_hash(&start_board) {
            let mut dfa_idx = 0;
            let mut board_idx = 0;
            //Find the location of the changed board in the DFA
            while board_idx < new_board.len() && dfa_idx < self.trans_table.len() {
                dfa_idx = self.trans_table[dfa_idx][new_board[board_idx] as usize];
                board_idx += 1;
            }
            if self.bfs_solver_sub(&new_board, dfa_idx, self.symbol_set.find_in_sig_set(new_board[board_idx..].iter()),investigated) {
                self.sig_sets[state_idx].set(sig_idx,true);
                break
            }
        }
        self.solved_yet[state_idx - self.trans_table.len()].set(sig_idx,true);
        self.sig_sets[state_idx][sig_idx]
    }

    fn bfs_solver(&mut self, start_board : &Vec<SymbolIdx>) -> bool {
        let mut start_idx = 0;
        let mut end_idx = 0;
        let mut all_boards : Vec<(usize,Vec<SymbolIdx>)> = vec![(0,start_board.clone())];
        let mut known_states = HashSet::<Vec<SymbolIdx>>::new();
        known_states.insert(start_board.clone());
        let mut answer_idx = 0;
        let mut answer_found = false;
        while (start_idx != end_idx || start_idx == 0) && !answer_found{
            start_idx = end_idx;
            end_idx = all_boards.len();
            for board_idx in start_idx..end_idx{
                if self.goal.contains(&all_boards[board_idx].1) {
                    answer_idx = board_idx;
                    answer_found = true;
                    break;
                }
                if let Some(found_answer) = self.board_solutions.get(&all_boards[board_idx].1) {
                    if !*found_answer {
                        continue
                    }else{
                        answer_idx = board_idx;
                        answer_found = true;
                        break;
                    }
                }
                for new_board in self.rules.single_rule_hash(&all_boards[board_idx].1) {
                    if !known_states.contains(&new_board) {
                        known_states.insert(new_board.clone());
                        all_boards.push((board_idx,new_board));
                    }
                }
            }
        }
        //did we find an answer board
        match answer_found{
            false => {
                //if it's unsolvable, then we know everything here is
            while let Some((_,board)) = all_boards.pop() {
                self.board_solutions.insert(board,false);
            }
            false
            }
            //this can be dramatically improved i think
            //following path of solvability
            true => {
                while answer_idx != 0 {
                    self.board_solutions.insert(all_boards[answer_idx].1.clone(),true);
                    answer_idx = all_boards[answer_idx].0;
                }
                self.board_solutions.insert(all_boards[0].1.clone(),true);
            true
            }
        }
    }
    fn sig_with_set(&mut self, board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>) -> Vec<bool> {
        let mut result : Vec<bool> = Vec::new();
        for sig_element in sig_set {
            let mut new_board = board.clone();
            new_board.extend(sig_element);
            result.push(self.bfs_solver(&new_board));
        }
        result
    }

    fn sig_with_set_sub(&mut self, board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>, state_idx : usize) {
        let solved_idx = state_idx - self.trans_table.len();
        let mut investigated = HashSet::new();
        for (idx,sig_element) in sig_set.iter().enumerate() {
            if !self.solved_yet[solved_idx][idx] {
                let mut new_board = board.clone();
                new_board.extend(sig_element);
                let result = self.bfs_solver_sub(&new_board, state_idx, idx, &mut investigated);
                investigated.clear();
            }
        }
    }

    fn sig_with_set_reverse(&mut self, board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>, accepted_boards : &HashSet<Vec<SymbolIdx>>) -> Vec<bool> {
        let mut result : Vec<bool> = Vec::with_capacity(sig_set.len());
        for sig_element in sig_set {
            let mut new_board = board.clone();
            new_board.extend(sig_element);
            result.push(accepted_boards.contains(&new_board));
        }
        result
    }

    fn sig_with_set_batch(&self, board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>, input : Arc<SegQueue<(Vec<SymbolIdx>,usize)>>, output : Arc<SegQueue<(bool,usize)>>) -> Vec<bool> {
        let mut result : Vec<bool> = vec![false;sig_set.len()];

        for sig_element in sig_set.iter().enumerate() {
            let mut new_board = board.clone();
            new_board.extend(sig_element.1);
            input.push((new_board,sig_element.0));
        }
        let mut results_recieved = 0;
        while results_recieved < sig_set.len() {
            match output.pop() {
                Some(output_result) => {
                    result[output_result.1] = output_result.0; 
                    results_recieved+=1;
                },
                None => {std::thread::sleep(time::Duration::from_millis(10));}
            }
        }
        result
    }

    fn board_to_next_batch(&self,board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>,input : &Arc<SegQueue<(Vec<SymbolIdx>,usize)>>, output : &Arc<SegQueue<(bool,usize)>>) -> Vec<(Vec<bool>,Vec<SymbolIdx>)> {
        let mut results : Vec<(Vec<bool>,Vec<SymbolIdx>)> = Vec::with_capacity(self.symbol_set.length);
        for sym in 0..(self.symbol_set.length as SymbolIdx) {
            let mut new_board = board.clone();
            new_board.push(sym);
            results.push((self.sig_with_set_batch(&new_board,sig_set,input.clone(),output.clone()),new_board));

        }
        results
    }

    fn board_to_next(&mut self,board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>) -> Vec<(Vec<bool>,Vec<SymbolIdx>)> {
        let mut results : Vec<(Vec<bool>,Vec<SymbolIdx>)> = Vec::with_capacity(self.symbol_set.length);
        for sym in 0..(self.symbol_set.length as SymbolIdx) {
            let mut new_board = board.clone();
            new_board.push(sym);
            results.push((self.sig_with_set(&new_board,sig_set),new_board));

        }
        results
    }
    fn board_to_next_reverse(&mut self,board : &Vec<SymbolIdx>, sig_set : &Vec<Vec<SymbolIdx>>, accepted_boards : &HashSet<Vec<SymbolIdx>>) -> Vec<(Vec<bool>,Vec<SymbolIdx>)> {
        let mut results : Vec<(Vec<bool>,Vec<SymbolIdx>)> = Vec::with_capacity(self.symbol_set.length);
        for sym in 0..(self.symbol_set.length as SymbolIdx) {
            let mut new_board = board.clone();
            new_board.push(sym);
            results.push((self.sig_with_set_reverse(&new_board,sig_set,accepted_boards),new_board));

        }
        results
    }

    fn dfa_with_sig_set_batch(&self, sig_set : &Vec<Vec<SymbolIdx>>) -> DFA {
        let mut trans_table : Vec<Vec<usize>> = Vec::new(); //omg it's me !!!
        let mut table_reference = HashMap::<Vec<bool>,usize>::new();
    
        let mut new_boards : Vec::<(usize,Vec<SymbolIdx>)> = vec![(0,vec![])];
    
        let mut old_boards : Vec::<(usize,Vec<SymbolIdx>)> = Vec::new();
    
        let mut accepting_states : HashSet<usize> = HashSet::new();
        
        let thread_translator : Arc<SRSTranslator> = Arc::new(self.clone());

        let (input, output) = self.create_workers(WORKERS);

        let mut empty_copy : Vec<usize> = Vec::new();
        for _ in 0..self.symbol_set.length {
            empty_copy.push(0);
        }

        let start_accepting = self.sig_with_set_batch(&vec![],&sig_set, input.clone(), output.clone());
        table_reference.insert(start_accepting.clone(),0);
        trans_table.push(empty_copy.clone());

        //redundant bc of start_accepting already checking this but idc
        if self.goal.contains(&vec![]) {
            accepting_states.insert(0);
        }
    
        while new_boards.len() > 0 {
            let iter_begin_time = Instant::now();
            std::mem::swap(&mut old_boards,&mut new_boards);
            new_boards.clear(); 
            println!("Thinking about {} states...",old_boards.len());
            print!("{} States | Length {} |",old_boards.len(),old_boards[0].1.len());
    
            for (start_idx,board) in &old_boards {
                //Finds ingoing end of board.
                
                //Gets sig set of all boards with a single symbol added.
                //TODO: Use pool of worker threads used with main-thread-blocking sig set requests.
                //Change Translator to a trait and add a batch SRSTranslator and a hash SRSTranslator.
                let next_results = self.board_to_next_batch(&board, sig_set, &input, &output);
                for (sym_idx,new_board) in next_results.iter().enumerate() {
                    //Checking if the next board's sig set already exists in DFA
                    let dest_idx = match table_reference.get(&new_board.0) {
                        //If it does, the arrow's obv going to the existing state in the DFA
                        Some(idx) => {
                            *idx
                        },
                        //If it doesn't, add a new state to the DFA!
                        None => {
                            let new_idx = trans_table.len();
                            new_boards.push((new_idx,new_board.1.clone()));
                            
                            
                            table_reference.insert(new_board.0.clone(),new_idx);
                            trans_table.push(empty_copy.clone());
    
                            if thread_translator.bfs_solver_batch(&new_board.1) {
                                accepting_states.insert(new_idx);
                            }
                            new_idx
                            }
                        };
                    trans_table[*start_idx][sym_idx] = dest_idx;
                    }  
                    
                }
                println!(" {} ms to complete",iter_begin_time.elapsed().as_millis());
            }
    
    self.terminate_workers(input, WORKERS);
    DFA {
        state_transitions : trans_table,
        accepting_states : accepting_states,
        starting_state : 0,
        symbol_set : self.symbol_set.clone()
    }
}
    fn create_workers(&self, worker_count : usize) -> (Arc<SegQueue<(Vec<SymbolIdx>,usize)>>,Arc<SegQueue<(bool,usize)>>) {
        let thread_translator : Arc<SRSTranslator> = Arc::new(self.clone());

        let input : Arc<SegQueue<(Vec<SymbolIdx>,usize)>> = Arc::new(SegQueue::new());
        let output : Arc<SegQueue<(bool,usize)>> = Arc::new(SegQueue::new());

        for _ in 0..worker_count {
            worker_thread(thread_translator.clone(), input.clone(), output.clone());
        }
        (input, output)
    }
    fn terminate_workers(&self, input : Arc<SegQueue<(Vec<SymbolIdx>,usize)>>, worker_count : usize) {
        for _ in 0..worker_count {
            input.push((vec![69, 42], usize::MAX))
        }
    }

    fn dfa_with_sig_set(&mut self, sig_set : &Vec<Vec<SymbolIdx>>) -> DFA {
        let mut trans_table : Vec<Vec<usize>> = Vec::new(); //omg it's me !!!
        let mut table_reference = HashMap::<Vec<bool>,usize>::new();
    
        let mut new_boards : Vec::<(usize,Vec<SymbolIdx>)> = vec![(0,vec![])];
    
        let mut old_boards : Vec::<(usize,Vec<SymbolIdx>)> = Vec::new();
    
        let mut accepting_states : HashSet<usize> = HashSet::new();
        

        let mut empty_copy : Vec<usize> = Vec::new();
        for _ in 0..self.symbol_set.length {
            empty_copy.push(0);
        }

        let start_accepting = self.sig_with_set(&vec![],&sig_set);
        table_reference.insert(start_accepting.clone(),0);
        trans_table.push(empty_copy.clone());

        //redundant bc of start_accepting already checking this but idc
        if self.bfs_solver(&vec![]) {
            accepting_states.insert(0);
        }
    
        while new_boards.len() > 0 {
            let iter_begin_time = Instant::now();
            std::mem::swap(&mut old_boards,&mut new_boards);
            new_boards.clear(); 
            print!("{} States | Length {} |",old_boards.len(),old_boards[0].1.len());
            //Horrific hack for 3xk boards. godspeed soldier
            self.board_solutions = HashMap::new();
            for (start_idx,board) in &old_boards {
                //Finds ingoing end of board.
                
                //Gets sig set of all boards with a single symbol added.
                let next_results = self.board_to_next(&board, sig_set);
                for (sym_idx,new_board) in next_results.iter().enumerate() {
                    //Checking if the next board's sig set already exists in DFA
                    let dest_idx = match table_reference.get(&new_board.0) {
                        //If it does, the arrow's obv going to the existing state in the DFA
                        Some(idx) => {
                            *idx
                        },
                        //If it doesn't, add a new state to the DFA!
                        None => {
                            let new_idx = trans_table.len();
                            new_boards.push((new_idx,new_board.1.clone()));
                            
                            
                            table_reference.insert(new_board.0.clone(),new_idx);
                            trans_table.push(empty_copy.clone());
    
                            if self.bfs_solver(&new_board.1) {
                                accepting_states.insert(new_idx);
                            }
                            new_idx
                            }
                        };
                    trans_table[*start_idx][sym_idx] = dest_idx;
                    }  
                    
                }
                println!(" {} ms",iter_begin_time.elapsed().as_millis());
            }
    DFA {
        state_transitions : trans_table,
        accepting_states : accepting_states,
        starting_state : 0,
        symbol_set : self.symbol_set.clone()
    }
    
}

fn dfa_with_sig_set_subset(&mut self, sig_set_size : usize) -> DFA {


    //graph of connections based on LHS->RHS links for all states
    //Usize is index in trans_table
    
    
    let sig_set = &self.build_sig_k(sig_set_size);

    //not allowed to complain about my dumb code -- not everything will be optimal i have DEADLINES.
    //okay i'm the one making up the deadlines... but still
    let smaller_sig = self.build_sig_k(sig_set_size - 1);

    //list of strings for the newest known states

    let mut recent_strings = vec![vec![]];

    let mut new_recent_strings = vec![];

    self.solved_yet.push(bitvec![0;sig_set.len()]);

    self.sig_sets.push(bitvec![0;sig_set.len()]);
    let mut start_values = bitvec![0;sig_set.len()];
    self.sig_with_set_sub(&vec![], &sig_set, 0);
    self.trans_table.push((1..=self.symbol_set.length).collect());
    self.unique_sigs.insert(self.sig_sets[0].clone(),0);

    self.solved_yet = vec![];

    //number of known states at last pass
    let mut last_known : usize = 1;
    //number of states with finished edges
    let mut last_finished : usize = 0;
    let mut update_string = "".to_owned();

    
     //while there are still states to process
 
    while last_finished < last_known{
        update_string = "".to_owned();

        let begin_time = Instant::now();

        update_string += &format!("{} States | ", last_known-last_finished);
        print!("{}\r",update_string);
        io::stdout().flush().unwrap();

        //First step is populating self.sig_sets and self.solved_yet 
        
        //trans_table should already be correct? make sure to that when adding elements
        let new_states = (last_known - last_finished) * self.symbol_set.length;
        self.sig_sets.resize(self.sig_sets.len()+new_states,bitvec![0;sig_set.len()]);
        self.solved_yet.resize(new_states,bitvec![0;sig_set.len()]);

        //next is adding all edges appropriately to the graph. 
        //this can be optimized substantially but i don't wanna do it pre-emptively :)
        let mut link_graph = DiGraph::<usize,()>::new();

        for index in 0..(last_known + new_states) {
            link_graph.add_node(index);
        }
        for origin in 0..last_known {
            for rule in &self.rules.rules {
                let mut parent = origin;
                let mut child = origin;
                let mut valid = true;
                for i in 0..rule.0.len() {
                    if parent >= last_known || child >= last_known {
                        valid = false;
                        break;
                    }
                    parent = self.trans_table[parent][rule.0[i] as usize];
                    child = self.trans_table[child][rule.1[i] as usize];
                }
                if valid {
                    link_graph.update_edge(NodeIndex::new(parent),NodeIndex::new(child),());
                }
            }  
        }
        // After establishing the starting points of all links, extend those links outward.
        let mut old_len = 0;
        while old_len < link_graph.edge_count() {
            let new_len = link_graph.edge_count();
            for edge_idx in old_len..new_len {
                for sym in 0..self.symbol_set.length {
                    let old_parent = link_graph[link_graph.raw_edges()[edge_idx].source()];
                    let old_child = link_graph[link_graph.raw_edges()[edge_idx].target()];
                    if old_parent >= last_known || old_child >= last_known {
                        continue
                    }
                    let new_parent = self.trans_table[old_parent][sym as usize];
                    let new_child = self.trans_table[old_child][sym as usize];
                    link_graph.update_edge(NodeIndex::new(new_parent),NodeIndex::new(new_child),());
                }
            }
            old_len = new_len;
        }

        //Next we implant the sig set info from previous states' into the prospective states.

        
        for origin_idx  in last_finished..last_known {
            for (sym,move_idx) in self.trans_table[origin_idx].iter().enumerate() {
                for elem in &smaller_sig {
                    let mut elem_in_origin = vec![sym as u8];
                    elem_in_origin.extend(elem.iter());
                    let old_idx = self.symbol_set.find_in_sig_set(elem_in_origin.iter());
                    let new_idx = self.symbol_set.find_in_sig_set(elem.iter());
                    let scared_rust = self.sig_sets[origin_idx][old_idx];
                    self.sig_sets[*move_idx].set(new_idx,scared_rust);
                    self.solved_yet[move_idx - last_known].set(new_idx,true);
                }
            }
        }
        
        //cycle detection and removal. note that this changes the type of node_weight from usize to Vec<usize>. 
        //tests indicate that this vec is always sorted smallest to largest, but this fact may not hold true if code is modified.
        let initial_nodes = link_graph.node_count();
        let link_graph = condensation(link_graph, true);

        update_string += &format!("{} Links | {} Cyclic duplicates | ", link_graph.edge_count(),initial_nodes - link_graph.node_count());
        print!("{}\r",update_string);
        io::stdout().flush().unwrap();
        //Next is updating prospective states with all known information.
        //We're intentionally leaning more heavily on solving ANY POSSIBLE strings ahead of time,
        //this operation is constant* which is MUCH better than O(x^n), so idgaf
        //*the cache does inevitably get rawdogged
        

        //This doesn't change the number of sig elements that get skipped at all ???
        for origin_node in link_graph.node_indices() {
            //Parent is known & child is unknown
            //This updates the value of impossible entries as
            //1. known 2. to be impossible 3. for the child
            //Notably, this never modifies the child's sig set! that's bc it starts as false anyway
            //also commented out bc it should be redundant

            let origin = link_graph[origin_node][0];
            if origin >= last_known {
                continue
            }

            let mut visit = HashSet::new();
            visit.insert(origin_node);
            let mut explore = vec![origin_node];
            //This updates the value of possible entries as
            //1. known 2. to be possible 3. for the parent
            
            while let Some(nx) = explore.pop() {
                for neighbor in link_graph.neighbors_directed(nx,Direction::Incoming) {
                    if link_graph[neighbor][0] >= last_known && !visit.contains(&neighbor) {
                        visit.insert(neighbor);
                        explore.push(neighbor);

                        //Unsure why these need to be cloned! hopefully it is nothing horrible 😅
                        self.solved_yet[link_graph[neighbor][0] - last_known] |= self.sig_sets[origin].clone();
                        let why = self.sig_sets[origin].clone();
                        self.sig_sets[link_graph[neighbor][0]] |= why;
                    }
                }
            }
        }
        
        //Known-unknown pairs are finally fucking over. Now it's time for the scariest --
        //Unknown-unknown.
        let process_begin_time = Instant::now();
        let mut processed_states = 0;
        let mut skipped_strings = 0;
        let mut reverse_link_graph = link_graph.clone();
        reverse_link_graph.reverse();
        for node in toposort(&reverse_link_graph, None).unwrap() {
            if link_graph[node][0] >= last_known {
                //Get info about what's false from all incoming neighbors
                for neighbor in link_graph.neighbors_directed(node,Direction::Incoming) {
                    //if the neighbor is a known state
                    if link_graph[neighbor][0] < last_known {
                        //everything that the sig set says is false for neighbor, is false for node
                        self.solved_yet[link_graph[node][0] - last_known] |= !self.sig_sets[link_graph[neighbor][0]].clone();
                    
                    }
                    //if the neighbor's also a prospective state
                    else{
                        //everything that's been solved and is false for neighbor, is false for node
                        let scared_rust = self.solved_yet[link_graph[neighbor][0] - last_known].clone();
                        self.solved_yet[link_graph[node][0] - last_known] |= 
                        (!self.sig_sets[link_graph[neighbor][0]].clone() & scared_rust);
                    }
                }
                //creating a string to actually test with
                let connecting_state = (link_graph[node][0] - last_known) / self.symbol_set.length;
                let connecting_symbol = ((link_graph[node][0] - last_known) % self.symbol_set.length) as SymbolIdx;
                let mut new_board = recent_strings[connecting_state].clone();
                new_board.push(connecting_symbol);
                skipped_strings += self.solved_yet[link_graph[node][0]- last_known].count_ones();
                self.sig_with_set_sub(&new_board, &sig_set, link_graph[node][0]);
                processed_states += 1;
                print!("{}{}/{} Skipped | {}/{} Calculated\r",update_string,skipped_strings, processed_states * sig_set.len(), processed_states,new_states);
                io::stdout().flush().unwrap();
            }
        }

        update_string += &format!("{}/{} Skipped | ~{:.3} ms per string | ", skipped_strings, processed_states * sig_set.len(), 
            ((Instant::now() -process_begin_time).as_millis() as f64) / ((processed_states * sig_set.len() - skipped_strings) as f64));
        print!("{}\r",update_string);
        io::stdout().flush().unwrap();
        //println!("{:?}",self.sig_sets[0]);
        //Now, we look at all prospective states' signature sets and add the unique ones.
        let mut new_known = 0;
        let mut new_sig_sets = vec![];
        let mut new_identified = 0;
        for pros_state in link_graph.node_indices() {
            //If there's an equivalent state that already exists in the DFA, use that!
            let connector = match link_graph[pros_state].iter().find(|&x| x < &last_known) {
                Some(idx) => {
                    *idx
                },
                None => {
                    print!("{}{}/{} Identified\r",update_string,new_identified,new_states);
                    new_identified += 1;
                    match self.unique_sigs.get(&self.sig_sets[link_graph[pros_state][0]]) {
                        Some(i) => {*i}
                        None => {
                            let connecting_state = (link_graph[pros_state][0] - last_known) / self.symbol_set.length + last_finished;
                            let connecting_symbol = ((link_graph[pros_state][0] - last_known) % self.symbol_set.length) as SymbolIdx;
                            let mut new_board = recent_strings[connecting_state - last_finished].clone();
                            new_board.push(connecting_symbol);
                            self.unique_sigs.insert(self.sig_sets[link_graph[pros_state][0]].clone(),new_known+last_known);
                            new_known += 1;
                            new_sig_sets.push(self.sig_sets[link_graph[pros_state][0]].clone());
                            new_recent_strings.push(new_board);
                            new_known+last_known-1
                        }
                    }
                    
                }
            };
            for dupe in &link_graph[pros_state] {
                if *dupe < last_known {
                    continue
                }
                let connecting_state = (dupe - last_known) / self.symbol_set.length + last_finished;
                let connecting_symbol = ((dupe - last_known) % self.symbol_set.length);
                self.trans_table[connecting_state][connecting_symbol] = connector;
            }
        }



        //Now we clean up -- no prospective states left over anywhere!

        self.sig_sets.truncate(last_known);
        self.sig_sets.append(&mut new_sig_sets);

        self.solved_yet.clear();

        for i in 0..new_known {
            self.trans_table.push(((last_known+new_known+i*self.symbol_set.length)..=(last_known+new_known+(i+1)*self.symbol_set.length-1)).collect())
        }
        last_finished = last_known;
        last_known = self.trans_table.len();

        std::mem::swap(&mut recent_strings, &mut new_recent_strings);
        new_recent_strings.clear();
        println!("{}{} ms               ", update_string,(Instant::now()-begin_time).as_millis());
    }
    let mut accepting_states = HashSet::new();
    for (key, val) in self.unique_sigs.iter() {
        if key[0] {
            accepting_states.insert(*val);
        }
    }
    let trans_table = self.trans_table.clone();
    self.trans_table = vec![];
    self.unique_sigs = HashMap::new();
    self.solved_yet = vec![];
    //self.sig_sets = vec![]; BAD AND TEMPORARY
    self.ss_link_graph = Graph::new();
    self.goal_minkids = vec![];
    self.ss_idx_to_link = vec![];
    DFA {
        state_transitions : trans_table,
        accepting_states : accepting_states,
        starting_state : 0,
        symbol_set : self.symbol_set.clone()
    }

}
//Rumplestiltsken Suavigion
fn dfa_with_sig_set_minkid(&mut self, sig_set_size : usize) -> DFA {


    //graph of connections based on LHS->RHS links for all states
    //Usize is index in trans_table
    let sig_set = &self.build_sig_k(sig_set_size);
    self.build_ss_link_graph(sig_set_size, sig_set);
    let mut dfa_graph = DiGraph::<MKDFAState,SymbolIdx>::new();
    let mut link_graph = DiGraph::<(), (Vec<SymbolIdx>,Vec<SymbolIdx>)>::new();
    dfa_graph.add_node(MKDFAState { minkids: self.goal_minkids[self.goal.starting_state].clone(), goal_states: vec![self.goal.starting_state] });
    link_graph.add_node(());
    //number of nodes after an iteration.
    //Each iteration only works if there are two lengths -- so we start with two.
    let mut iteration_lens = vec![0,1];

    //While new elements are actually getting added to the DFA
    while iteration_lens[iteration_lens.len() - 2] < iteration_lens[iteration_lens.len() - 1] {
        println!("iteration {}", iteration_lens.len() - 2);
        //First, adding all prospective DFA elements
        //This only adds nodes to the most recent iteration of DFA elements
        for start_idx in iteration_lens[iteration_lens.len() - 2]..iteration_lens[iteration_lens.len() - 1] {
            //Root node that prospective state will be connected to
            let start_node = NodeIndex::new(start_idx);
            for next_sym in 0..(self.symbol_set.length as SymbolIdx) {
                //states that the prospective state can reach into the goal DFA
                let mut goal_connections = vec![];
                //Set of minimum kids that can be added without any SRS applications.
                let mut minkids = HashSet::new();
                //For each goal DFA state that the root node can reach,
                for start_connection in &dfa_graph[start_node].goal_states {
                    //Add where the DFA goes after the input symbol that defines the connection between root and prospective DFA state
                    let new_connect = self.goal.state_transitions[*start_connection][next_sym as usize];
                    //Don't add anything twice. Seems like that'd be trouble.
                    if !goal_connections.contains(&new_connect){
                        //If that connection to the goal DFA hasn't already been made,
                        //add to our vec of reachable states,
                        goal_connections.push(new_connect);

                        //And add the minkids that don't require SRS applications.
                        if minkids.is_empty() {
                            minkids = self.goal_minkids[new_connect].clone();
                        }else{
                            self.add_set_to_minkids(&mut minkids, &self.goal_minkids[new_connect]);
                        }
                    }
                }
                let new_node = dfa_graph.add_node(MKDFAState { minkids: minkids, goal_states: goal_connections });
                link_graph.add_node(());
                dfa_graph.add_edge(start_node, new_node, next_sym);
            }
        }

        //Next, we BUILD the LINK GRAPH !!! (this should inspire fear)
        //This also has some major room for effiency improvements imo
        //but it wasn't really noticable for the subset implementation? 
        //will check perf later (ofc)
        
        //The range here can only possibly include elements max_input away from the diameter,
        //as otherwise we know that any connections they possess must have been added before.
        //In fact, this should probably be abused to cull old elements that cannot possibly add new info
        //But again... that's for later! ... and it assumes I don't end up building something completely new again :/
        let mut underflow_dodge = 0;
        if self.rules.max_input < iteration_lens.len() - 1 {
            underflow_dodge = iteration_lens.len() - self.rules.max_input - 1;
        }
        //Not constantly reinitializing this for optimization reasons
        for start_idx in iteration_lens[underflow_dodge]..iteration_lens[iteration_lens.len() - 1] {
            //Root node that prospective state will be connected to
            let start_node = NodeIndex::new(start_idx);
            //ONLY WORKS FOR RULES OF EQUAL LENGTH -- HONESTLY, THINKING ABOUT DELETING/GENERATING RULES MAKES MY HEAD HURT
            for rule in &self.rules.rules {
                let mut lhs = start_node;
                let mut rhs = start_node;
                let mut p_rule_len = 0;
                while p_rule_len < rule.0.len() {
                    p_rule_len += 1;
                    //If both the rhs and the lhs can actually go further in the DFA
                    if let Some(new_lhs_edge) = dfa_graph.edges_directed(lhs,Outgoing).find(|x| *x.weight() == rule.0[p_rule_len-1]) {
                        if let Some(new_rhs_edge) = dfa_graph.edges_directed(rhs,Outgoing).find(|x| *x.weight() == rule.1[p_rule_len-1]) {
                            lhs = new_lhs_edge.target();
                            rhs = new_rhs_edge.target();
                            self.add_link(&mut link_graph, lhs, rhs, &rule.0[p_rule_len..], &rule.1[p_rule_len..]);
                        }else {
                            break
                        }
                    }
                    else {
                        break
                    }
                    
                } 
            }
        }
        //Realized I am dumb as bricks! We need to propagate pure connections!!!
        //DUH!!!!!
        //Currently crawls the entire fucking link graph bc i am dumb and tired and really curious
        for start_idx in 0..iteration_lens[iteration_lens.len() - 1] {
            let start_node = NodeIndex::new(start_idx);
            let mut possible_edge =  link_graph.first_edge(start_node, Outgoing);
            while let Some(real_edge) = possible_edge {
                possible_edge = link_graph.next_edge(real_edge, Outgoing);
                let target = link_graph.edge_endpoints(real_edge).unwrap().1;
                if !(link_graph[real_edge].0.is_empty() && link_graph[real_edge].1.is_empty() && target.index() < iteration_lens[iteration_lens.len() - 1]) {
                    continue
                }
                let lhs = start_node;
                let rhs = target;
                let mut propagation_pairs = vec![(lhs,rhs)];
                while let Some(prop_pair) = propagation_pairs.pop() {
                    for sym in 0..self.symbol_set.length {
                        let mut lhs_extension = lhs;
                        let mut rhs_extension = rhs;
                        if let Some(lhs_edge) = dfa_graph.edges_directed(prop_pair.0,Outgoing).find(|x| *x.weight() == sym as SymbolIdx) {
                            lhs_extension = lhs_edge.target();
                        } else {
                            continue
                        }
                        if let Some(rhs_edge) = dfa_graph.edges_directed(prop_pair.1,Outgoing).find(|x| *x.weight() == sym as SymbolIdx) {
                            rhs_extension = rhs_edge.target();
                        } else {
                            continue
                        }
                        if link_graph.edges_connecting(lhs_extension, rhs_extension).any(|x| x.weight().0.is_empty() && x.weight().1.is_empty() ) {
                            continue;
                        }
                        let mut death_row = vec![];
                        for dead_edge in link_graph.edges_connecting(lhs_extension, rhs_extension) {
                            if !(dead_edge.weight().0.is_empty() && dead_edge.weight().1.is_empty()) {
                                death_row.push(dead_edge.id());
                            }
                        }
                        for dead_edge in death_row {
                            link_graph.remove_edge(dead_edge);
                        }
                        if lhs != lhs_extension || rhs != rhs_extension {
                            link_graph.add_edge(lhs_extension, rhs_extension, (vec![],vec![]));
                            propagation_pairs.push((lhs_extension,rhs_extension));
                        }
                    }
                }
            }
        }

        let mut debug_link_graph : DiGraph<String,(Vec<SymbolIdx>,Vec<SymbolIdx>)> = Graph::new();
        for i in 0..link_graph.node_count() {
            if i < *iteration_lens.last().unwrap() {
                debug_link_graph.add_node(format!("Known q{}",i));
            } else{
                debug_link_graph.add_node(format!("{} from q{}",
                    (i- *iteration_lens.last().unwrap())%self.symbol_set.length,
                    ((i- *iteration_lens.last().unwrap())/self.symbol_set.length + iteration_lens[iteration_lens.len() - 2])));
            }
        }
        for edge in link_graph.raw_edges() {
            debug_link_graph.add_edge(edge.source(), edge.target(), edge.weight.clone());
        }

        let mut file = File::create(format!("link_graph_debug/{}.dot",iteration_lens.len()-2)).unwrap();
        file.write_fmt(format_args!("{:?}",Dot::new(&debug_link_graph)));
        //Alright, pretending/assuming that we've written that correctly, we move on to actually propagating ancestors!
        //this also sucks :(
        //Just to get the ball rolling, we run through everything new once
        let mut affected_nodes = HashSet::new();
        for prospective_idx in *iteration_lens.last().unwrap()..dfa_graph.node_count() {
            let prospective_node = NodeIndex::new(prospective_idx);
            for edge in link_graph.edges_directed(prospective_node, Outgoing) {
                //If it modifies its source
                if self.partial_link(&mut dfa_graph, sig_set, edge.weight(), edge.source(), edge.target()) {
                    affected_nodes.insert(prospective_node);
                }
            }
        }
        //Continue propagating changes until no more exist!
        //This propagation could be better (do not add things to new list if they haven't been executed in current loop is the main one off the dome)
        let mut old_affected_nodes = HashSet::new();
        while !affected_nodes.is_empty() {
            old_affected_nodes.clear();
            std::mem::swap(&mut old_affected_nodes, &mut affected_nodes);
            for affected_node in &old_affected_nodes {
                for edge in link_graph.edges_directed(*affected_node, Incoming) {
                    //This should just be an optimization, as it implies an impossible thing. This is not why I have added it.
                    if edge.source().index() < *iteration_lens.last().unwrap() {
                        continue;
                    }
                    if self.partial_link(&mut dfa_graph, sig_set, edge.weight(), edge.source(), *affected_node) {
                        affected_nodes.insert(edge.source());
                    }
                }
            }
        }

        //Now, prune duplicates. Notably, there's no implementation of Hash on HashSets (extremely surprising to me), so hopefully this garbo solution doesn't take forever
        let mut new_count = 0;
        let mut prospective_state = *iteration_lens.last().unwrap();
        while prospective_state < dfa_graph.node_count() {
            
            let pros_node = NodeIndex::new(prospective_state);
            let mut equivalent_known = None;
            
            for known_state in 0..(*iteration_lens.last().unwrap()+new_count) {
                let state_node = NodeIndex::new(known_state);
                if dfa_graph[pros_node].minkids == dfa_graph[state_node].minkids {
                    equivalent_known = Some(state_node);
                    break
                }
            }
            match equivalent_known{
                Some(equiv) => {
                    //Re-link if there exists an equivalent state
                    let disappointed_parent_edge = dfa_graph.edges_directed(pros_node, Incoming).last().unwrap();
                    dfa_graph.add_edge(disappointed_parent_edge.source(), equiv,*disappointed_parent_edge.weight());

                    //Remove the duplicate from the graph
                    dfa_graph.remove_node(pros_node);

                    //Make sure to preserve any connections in the link graph!
                    let mut potential_incoming = link_graph.first_edge(pros_node, Incoming);
                    while let Some(real_incoming) = potential_incoming {
                        potential_incoming = link_graph.next_edge(real_incoming, Incoming);
                        let source = link_graph.edge_endpoints(real_incoming).unwrap().0;
                        let rust_scared = link_graph[real_incoming].clone();
                        self.add_link(&mut link_graph, source, equiv, &rust_scared.0[..], &rust_scared.1[..])
                    }

                    let mut potential_outgoing = link_graph.first_edge(pros_node, Outgoing);
                    while let Some(real_outgoing) = potential_outgoing {
                        potential_outgoing = link_graph.next_edge(real_outgoing, Outgoing);
                        let target = link_graph.edge_endpoints(real_outgoing).unwrap().1;
                        let rust_scared = link_graph[real_outgoing].clone();
                        self.add_link(&mut link_graph, equiv,target, &rust_scared.0[..], &rust_scared.1[..])
                    }
                    link_graph.remove_node(pros_node);
                    prospective_state -= 1;
                }
                None => {
                    //Otherwise, ensure we factor the new guy into our math
                    new_count += 1;
                }
            }
            prospective_state += 1;
        }
        iteration_lens.push(dfa_graph.node_count());
        //Oh god is that it?
        //I am terrified of facing the music
        //Original pass finished 7/24
        //Actually working pass ...
    }
    
    let mut trans_table = vec![vec![0;self.symbol_set.length];dfa_graph.node_count()];
    let mut accepting_states = HashSet::new();
    let mut minkids_debug = vec![];
    for node in dfa_graph.node_indices() {
        for edge in dfa_graph.edges_directed(node,Outgoing) {
            trans_table[node.index()][*edge.weight() as usize] = edge.target().index();
        }
        //Checks if empty string set is a member of minkids or an ancestor of it
        if self.check_if_ancestor(&dfa_graph[node].minkids, self.ss_idx_to_link[0]) {
            accepting_states.insert(node.index());
        }
        minkids_debug.push(self.minkids_to_tt(sig_set, &dfa_graph[node].minkids));
    }
    self.sig_sets = minkids_debug;
    DFA {
        state_transitions : trans_table,
        accepting_states : accepting_states,
        starting_state : 0,
        symbol_set : self.symbol_set.clone()
    }

}

fn dfa_with_sig_set_reverse(&mut self, sig_set : &Vec<Vec<SymbolIdx>>) -> DFA {
    let mut trans_table : Vec<Vec<usize>> = Vec::new(); //omg it's me !!!
    let mut table_reference = HashMap::<Vec<bool>,usize>::new();

    let mut new_boards : Vec::<(usize,Vec<SymbolIdx>)> = vec![(0,vec![])];

    let mut old_boards : Vec::<(usize,Vec<SymbolIdx>)> = Vec::new();

    let mut accepting_states : HashSet<usize> = HashSet::new();
    

    let mut empty_copy : Vec<usize> = Vec::new();
    for _ in 0..self.symbol_set.length {
        empty_copy.push(0);
    }

    let start_accepting = self.sig_with_set(&vec![],&sig_set);
    table_reference.insert(start_accepting.clone(),0);
    trans_table.push(empty_copy.clone());

    //redundant bc of start_accepting already checking this but idc
    if self.bfs_solver(&vec![]) {
        accepting_states.insert(0);
    }
    let mut accepted_boards : HashSet<Vec<SymbolIdx>> = HashSet::new();
    while new_boards.len() > 0 {
        let iter_begin_time = Instant::now();
        std::mem::swap(&mut old_boards,&mut new_boards);
        new_boards.clear(); 
        println!("{} States to think about...",old_boards.len());
        print!("{} States | Length {} |",old_boards.len(),old_boards[0].1.len());
        //Horrific hack for 3xk boards. godspeed soldier
        //5 HERE IS ALSO A HORRIFIC HACK. WE ARE BEYOND THE LOOKING GLASS. WE ARE FIGHTING FOR SURVIVAL.
         
        let mut starting_boards = vec![];
        for masta in (old_boards[0].1.len()+1)..(old_boards[0].1.len()+7) {
            for l in 0..masta {
                let mut prefix = vec![];
                let mut suffix = vec![];
                for v in 0..l { //0, 1, 2, 3, 4, 5
                    prefix.push(0);
                }
                for v in (l+1)..masta{  //5, 4, 3, 2, 1, 0
                    suffix.push(0);
                }
                for v in &[1,2] {
                    let mut result = prefix.clone();
                    result.push(*v);
                    result.extend(&suffix);
                    starting_boards.push(result.clone());
                    accepted_boards.insert(result);
                }
                
            }
        }
        //println!("{:?}",starting_boards);
        accepted_boards.retain(|k| k.len() > old_boards[0].1.len());
        self.rules.all_reverse_from(&starting_boards,&mut accepted_boards);
        let mut useful_prefixes = HashSet::<Vec<SymbolIdx>>::new();
        for (_,board) in &old_boards{
            useful_prefixes.insert(board.clone());
         }
    
        let iter_sig_time = Instant::now();

        for (start_idx,board) in &old_boards {
            //Finds ingoing end of board.
            
            //Gets sig set of all boards with a single symbol added.
            let next_results = self.board_to_next_reverse(&board, sig_set,&accepted_boards);
            for (sym_idx,new_board) in next_results.iter().enumerate() {
                //Checking if the next board's sig set already exists in DFA
                let dest_idx = match table_reference.get(&new_board.0) {
                    //If it does, the arrow's obv going to the existing state in the DFA
                    Some(idx) => {
                        *idx
                    },
                    //If it doesn't, add a new state to the DFA!
                    None => {
                        let new_idx = trans_table.len();
                        new_boards.push((new_idx,new_board.1.clone()));
                        
                        
                        table_reference.insert(new_board.0.clone(),new_idx);
                        trans_table.push(empty_copy.clone());

                        if accepted_boards.contains(&new_board.1) {
                            accepting_states.insert(new_idx);
                        }
                        new_idx
                        }
                    };
                trans_table[*start_idx][sym_idx] = dest_idx;
                }  
                
            }
            println!(" {} Accepting Boards | Board-Gen {} ms | Sig-Set {} ms | Total {} ms",
            accepted_boards.len(),
            (iter_sig_time-iter_begin_time).as_millis(),
            iter_sig_time.elapsed().as_millis(),
            iter_begin_time.elapsed().as_millis()
            );
        }
DFA {
    state_transitions : trans_table,
    accepting_states : accepting_states,
    starting_state : 0,
    symbol_set : self.symbol_set.clone()
}
}
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

} 

fn symbols_to_string(symbols : &Vec<SymbolIdx>) -> String{
    let mut string = "".to_owned();
    for sym in symbols {
        string += &format!("{}", sym);
    }
    string
}

impl PartialEq for DFA {
    fn eq(&self, other: &Self) -> bool {
        let mut stack = vec![(self.starting_state,other.starting_state)];
        let mut visited = HashSet::new();
        if self.symbol_set.length != other.symbol_set.length {
            return false;
        }
        visited.insert((self.starting_state,other.starting_state));
        while let Some(pair) = stack.pop() {
            if(self.accepting_states.contains(&pair.0) != other.accepting_states.contains(&pair.1)) {
                return false;
            }
            for i in 0..self.symbol_set.length {
                let test = (self.state_transitions[pair.0][i],other.state_transitions[pair.1][i]);
                if !visited.contains(&test) {
                    visited.insert(test.clone());
                    stack.push(test);
                }
            }
        }
        true
    }
}

impl DFA {

    fn ss_eq(&self, other: &Self, our_ss : &Vec<BitVec>, other_ss : &Vec<BitVec>) -> Vec<(usize,usize,Vec<usize>,Vec<usize>)> {
        let mut stack = HashSet::new();
        stack.insert((self.starting_state,other.starting_state));
        let mut old_stack = HashSet::new();
        let mut visited = HashSet::new();
        let mut result = vec![];
        visited.insert((self.starting_state,other.starting_state));
        while !stack.is_empty() {
            old_stack.clear();
            std::mem::swap(&mut old_stack, &mut stack);
            for pair in &old_stack {
                if(our_ss[pair.0] != other_ss[pair.1]) {
                    let mut too_nice = vec![];
                    let mut too_mean = vec![];
                    for bit in 0..our_ss[0].len() {
                        if our_ss[pair.0][bit] &&  !other_ss[pair.1][bit] {
                            too_nice.push(bit);
                        }else if !our_ss[pair.0][bit] && other_ss[pair.1][bit]{
                            too_mean.push(bit);
                        }
                    }
                    result.push((pair.0,pair.1,too_nice,too_mean));
                    continue
                }
                for i in 0..self.symbol_set.length {
                    let test = (self.state_transitions[pair.0][i],other.state_transitions[pair.1][i]);
                    if !visited.contains(&test) {
                        visited.insert(test.clone());
                        stack.insert(test);
                    }
                }
            }
        }
        result
    }

    fn contains(&self, input : &Vec<SymbolIdx>) -> bool {
        let mut state = self.starting_state;
        for i in input {
            state = self.state_transitions[state][(*i as usize)];
        }
        self.accepting_states.contains(&state)
    }

    fn final_state(&self, input : &Vec<SymbolIdx>) -> usize{
        let mut state = self.starting_state;
        for i in input {
            state = self.state_transitions[state][(*i as usize)];
        }
        state
    }

    fn contains_from_start(&self, input : &Vec<SymbolIdx>, start : usize) -> bool {
        let mut state = start;
        for i in input {
            state = self.state_transitions[state][(*i as usize)];
        }
        self.accepting_states.contains(&state)
    }

    fn shortest_path_to_state(&self, desired : usize) -> Vec<SymbolIdx> {
        if desired == self.starting_state {
            return vec![];
        }
        let mut backpath = vec![usize::MAX;self.state_transitions.len()];
        backpath[self.starting_state] = self.starting_state;
        let mut next_paths = vec![self.starting_state];
        let mut old_paths = vec![];
        let mut found_desired = false;
        while !found_desired && !next_paths.is_empty(){
            old_paths.clear();
            std::mem::swap(&mut next_paths, &mut old_paths);
            for frontier_state in &old_paths {
                for next_spot in &self.state_transitions[*frontier_state] {
                    if backpath[*next_spot] == usize::MAX {
                        backpath[*next_spot] = *frontier_state;
                        next_paths.push(*next_spot);
                        if *next_spot == desired {
                            found_desired = true;
                            break
                        }
                    }
                }
            }
        }
        let mut path : Vec<SymbolIdx> = vec![];
        let mut cur_state = desired;
        while cur_state != self.starting_state {
            let back_state = backpath[cur_state];
            for sym in 0..self.symbol_set.length {
                if self.state_transitions[back_state][sym] == cur_state {
                    path.push(sym as SymbolIdx);
                    break
                }
            }
            cur_state = back_state;
        }
        path.reverse();
        path

    }


    fn shortest_path_to_pair(&self, other: &Self, our_state : usize, other_state : usize) -> Vec<SymbolIdx> {
        let mut old_stack = HashSet::new();
        let mut stack = HashSet::new();
        stack.insert((self.starting_state,other.starting_state));
        let mut visited = HashMap::new();
        visited.insert((self.starting_state,other.starting_state),(self.starting_state,other.starting_state));
        while !stack.is_empty(){
            old_stack.clear();
            std::mem::swap(&mut old_stack, &mut stack);
            for pair in &old_stack {
                if *pair == (our_state, other_state) {
                    stack.clear();
                    break
                }
                for i in 0..self.symbol_set.length {
                    let test = (self.state_transitions[pair.0][i],other.state_transitions[pair.1][i]);
                    if !visited.contains_key(&test) {
                        visited.insert(test.clone(),pair.clone());
                        stack.insert(test);
                    }
                }
            }
        }

        let mut path : Vec<SymbolIdx> = vec![];
        let mut cur_state = (our_state,other_state);
        while cur_state != (self.starting_state,other.starting_state) {
            let back_state = visited[&cur_state];
            for sym in 0..self.symbol_set.length {
                if self.state_transitions[back_state.0][sym] == cur_state.0 && other.state_transitions[back_state.1][sym] == cur_state.1 {
                    path.push(sym as SymbolIdx);
                    break
                }
            }
            cur_state = back_state;
        }
        path.reverse();
        path
    }

    fn save_jflap_to_file(&self,file : &mut File) {
        let mut w = EmitterConfig::new().perform_indent(true).create_writer(file);
        w.write(XmlEvent::start_element("structure")).unwrap();
        w.write(XmlEvent::start_element("type")).unwrap();
        w.write(XmlEvent::characters("fa")).unwrap();
        w.write(XmlEvent::end_element()).unwrap();
        w.write(XmlEvent::start_element("automaton")).unwrap();
        
        for (idx,i) in self.state_transitions.iter().enumerate() {
            w.write(XmlEvent::start_element("state")
                                                                .attr("id",&idx.to_string())
                                                                .attr("name",&("q".to_owned()+&idx.to_string()))
                                                            ).unwrap();
            if idx == self.starting_state {
                w.write(XmlEvent::start_element("initial")).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
            }                                
            if self.accepting_states.contains(&idx) {
                w.write(XmlEvent::start_element("final")).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
            }
            w.write(XmlEvent::end_element()).unwrap();
        }
        let symbols = &self.symbol_set.representations;
        for (idx,state) in self.state_transitions.iter().enumerate() {
            for (idx2,target) in state.iter().enumerate() {
                w.write(XmlEvent::start_element("transition")).unwrap();
                w.write(XmlEvent::start_element("from")).unwrap();
                w.write(XmlEvent::characters(&idx.to_string())).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
                w.write(XmlEvent::start_element("to")).unwrap();
                w.write(XmlEvent::characters(&target.to_string())).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
                w.write(XmlEvent::start_element("read")).unwrap();
                w.write(XmlEvent::characters(&format!("{}",symbols[idx2]))).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
                w.write(XmlEvent::end_element()).unwrap();
            }

        }
        w.write(XmlEvent::end_element()).unwrap();
        w.write(XmlEvent::end_element()).unwrap();
    }

    fn jflap_save(&self, filename : &str) {
        let mut file = File::create(filename.clone().to_owned() + ".jff").unwrap();
        self.save_jflap_to_file(&mut file);
    }
    fn save(&self, filename : &str) {
        let mut file = File::create(filename.clone().to_owned() + ".dfa").unwrap();
        file.write(serde_json::to_string(self).unwrap().as_bytes());
    }
    fn load(filename : &str) -> Result::<Self> {
        let mut contents = fs::read_to_string(filename.clone().to_owned() + ".dfa").unwrap();
        serde_json::from_str(&contents)
    }
}


fn build_threerulesolver() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 3,
        representations : vec!["0".to_owned(),"1".to_owned(),"2".to_owned()]
    };
    let ruleset = Ruleset::new(
        vec![(vec![1,1,0],vec![0,0,1]),
                     (vec![0,1,1],vec![1,0,0]),
                     (vec![1,0,1],vec![0,1,0]),
                     (vec![2,1,0],vec![0,0,2]),
                     (vec![0,1,2],vec![2,0,0]),
                     (vec![2,0,1],vec![0,2,0]),
                     (vec![1,0,2],vec![0,2,0]),
        ],
        b_symbol_set.clone()
    );
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,2,1],vec![1,2,2],vec![2,2,2]],
        accepting_states : HashSet::from_iter(vec![1]),
        symbol_set : b_symbol_set.clone()
    };
    SRSTranslator::new(ruleset,goal_dfa)
}
fn build_defaultsolver() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 3,
        representations : vec!["0".to_owned(),"1".to_owned(),"2".to_owned()]
    };
    let ruleset = Ruleset::new(
        vec![(vec![1,1,0],vec![0,0,1]),
                     (vec![0,1,1],vec![1,0,0]),
                     (vec![2,1,0],vec![0,0,2]),
                     (vec![0,1,2],vec![2,0,0]),
        ],
        b_symbol_set.clone()
    );
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,2,1],vec![1,2,2],vec![2,2,2]],
        accepting_states : HashSet::from_iter(vec![1]),
        symbol_set : b_symbol_set.clone()
    };
    SRSTranslator::new(ruleset,goal_dfa)
}

fn build_2xnswap() -> SRSTranslator {
    let symbol_set = SymbolSet {
        length : 3,
        representations : vec!["0".to_owned(),"1".to_owned(),"2".to_owned()]
    };

    let mut rules : Vec::<(Vec<SymbolIdx>,Vec<SymbolIdx>)> = vec![];

    for i in 0..(8 as SymbolIdx) {
        let big = (i / 4) % 2;
        let mid = (i / 2) % 2;
        let sml = i % 2;
        rules.push((vec![1+big,1+mid,0+sml],vec![0+big,0+mid,1+sml]));
        rules.push((vec![0+big,1+mid,1+sml],vec![1+big,0+mid,0+sml]));
    }

    let ruleset = Ruleset::new(
        rules,
        symbol_set.clone()
    );

    let old_dfa = DFA::load("default1dpeg").unwrap();
    let mut new_transitions = vec![];

    let error_state = 10;
    for state in old_dfa.state_transitions {
        new_transitions.push(vec![state[0],state[1],error_state]);

    }
    let goal_dfa = DFA { 
        starting_state: 0, 
        state_transitions: new_transitions, 
        accepting_states: old_dfa.accepting_states, 
        symbol_set: symbol_set.clone()
     };
     SRSTranslator::new(ruleset,goal_dfa)
}

fn build_default1dpeg() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 2,
        representations : vec!["0".to_owned(),"1".to_owned()]
    };
    let ruleset = Ruleset::new(
        vec![(vec![1,1,0],vec![0,0,1]),
                     (vec![0,1,1],vec![1,0,0]),
        ],
        b_symbol_set.clone()
    );
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,1],vec![1,2],vec![2,2]],
        accepting_states : HashSet::from_iter(vec![1]),
        symbol_set : b_symbol_set.clone()
    };
    SRSTranslator::new(ruleset,goal_dfa)
}

fn build_threerule1dpeg() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 2,
        representations : vec!["0".to_owned(),"1".to_owned()]
    };
    let ruleset = Ruleset::new(
        vec![(vec![1,1,0],vec![0,0,1]),
                     (vec![0,1,1],vec![1,0,0]),
                     (vec![1,0,1],vec![0,1,0])
        ],
        b_symbol_set.clone()
    );
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,1],vec![1,2],vec![2,2]],
        accepting_states : HashSet::from_iter(vec![1]),
        symbol_set : b_symbol_set.clone()
    };
    SRSTranslator::new(ruleset,goal_dfa)
}

fn build_flip() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 2,
        representations : vec!["0".to_owned(),"1".to_owned()]
    };
    let mut rules_vec = vec![];
    for i in 0..8 {
        rules_vec.push((vec![i/4 % 2, i / 2 % 2, i % 2],vec![1-i/4 % 2, 1-i / 2 % 2, 1-i % 2]))
    }
    let ruleset = Ruleset::new(
        rules_vec,
        b_symbol_set.clone()
    );
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,1],vec![1,1]],
        accepting_states : HashSet::from_iter(vec![0]),
        symbol_set : b_symbol_set.clone()
    };
    SRSTranslator::new(ruleset,goal_dfa)
}

fn build_flipx3() -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 2,
        representations : vec!["0".to_owned(),"1".to_owned()]
    };
    let mut rules_vec = vec![];
    for i in 0..8 {
        rules_vec.push((vec![i/4 % 2, i / 2 % 2, i % 2],vec![1-i/4 % 2, 1-i / 2 % 2, 1-i % 2]))
    }
    let ruleset = Ruleset::new(
        rules_vec,
        b_symbol_set.clone()
    );
    
    let k = 3;
    let symbol_num = 2_u32.pow(k as u32) as usize;
    let mut new_rules = vec![];
    let mut vert_starts = vec![];
    let mut vert_ends = vec![];
    for rule in ruleset.rules {
        //Horizontal (single-symbol) rules
        for i in 0..(rule.0.len() - k+1) {
            let mut start_sym_idx = 0;
            for (rule_sym_idx, rule_sym) in rule.0.iter().enumerate() {
                start_sym_idx += rule_sym*(b_symbol_set.length as SymbolIdx).pow((rule.0.len()-rule_sym_idx-1) as u32);
            }
            start_sym_idx *= (b_symbol_set.length as SymbolIdx).pow((i) as u32);

            let mut end_sym_idx = 0;
            for (rule_sym_idx, rule_sym) in rule.1.iter().enumerate() {
                end_sym_idx += rule_sym*(b_symbol_set.length as SymbolIdx).pow((rule.1.len()-rule_sym_idx-1) as u32);
            }
            end_sym_idx *= (b_symbol_set.length as SymbolIdx).pow(i as u32);
            new_rules.push((vec![start_sym_idx],vec![end_sym_idx]));
        }
        //Vertical (normal symbol length) rules
        //i is horizontal index selected
        //Represents the fixed column we're doing business with
        
        
        for i in 0..k {
            //LHS and RHS respectively
            vert_starts = vec![vec![0;rule.0.len()]];
            vert_ends = vec![vec![0;rule.1.len()]];
            //j is vertical index selected
            for j in 0..k {
                let cur_vert_rules = vert_starts.len();
                let pow_num = (b_symbol_set.length as SymbolIdx).pow(j as u32);
                
                //If we're looking at the fixed row
                if i == j {
                    for start_idx in 0..cur_vert_rules {
                        for vert_idx in 0..vert_starts[start_idx].len() {
                            vert_starts[start_idx][vert_idx] += rule.0[vert_idx]*pow_num;
                        }
                        for vert_idx in 0..vert_ends[start_idx].len() {
                            vert_ends[start_idx][vert_idx] += rule.1[vert_idx]*pow_num;
                        }
                    }
                } else {
                    for start_idx in 0..cur_vert_rules {
                        for k in 1..symbol_num {
                            let mut new_vert_start = vert_starts[start_idx].clone();
                            let mut new_vert_end = vert_ends[start_idx].clone();
                            for l in 0..k {
                                if (k >> l) % 2 == 1 {
                                    new_vert_start[l] += pow_num;
                                    new_vert_end[l] += pow_num;
                                }
                            }
                            
                            vert_starts.push(new_vert_start);
                            vert_ends.push(new_vert_end);
                        }
                    }
                }
                
            }
            for i in 0..vert_starts.len() {
                new_rules.push((vert_starts[i].clone(),vert_ends[i].clone()));
            }
        }
    }
    let by_k_symbol_set = SymbolSet {
        length : 2_u32.pow(k as u32) as usize,
        representations : vec!["000".to_owned(),"001".to_owned(),"010".to_owned(),"011".to_owned(),"100".to_owned(),"101".to_owned(),"110".to_owned(),"111".to_owned()] //whoops! lol
    };
    
    let ruleset = Ruleset::new(new_rules,by_k_symbol_set.clone());
    
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : vec![vec![0,1,1,1,1,1,1,1],vec![1,1,1,1,1,1,1,1]],
        accepting_states : HashSet::from_iter(vec![0]),
        symbol_set : by_k_symbol_set.clone()
    };
    
    SRSTranslator::new(ruleset,goal_dfa)
}

fn build_default2dpegx3 () -> SRSTranslator {
    let b_symbol_set = SymbolSet {
        length : 2,
        representations : vec!["0".to_owned(),"1".to_owned()]
    };

    let ruleset = Ruleset::new(
        vec![(vec![1,1,0],vec![0,0,1]),
                     (vec![0,1,1],vec![1,0,0]),
        ],
        b_symbol_set.clone()
    );
    
    let k = 3;
    let symbol_num = 2_u32.pow(k as u32) as usize;
    let mut new_rules = vec![];
    let mut vert_starts = vec![];
    let mut vert_ends = vec![];
    for rule in ruleset.rules {
        //Horizontal (single-symbol) rules
        for i in 0..(rule.0.len() - k+1) {
            let mut start_sym_idx = 0;
            for (rule_sym_idx, rule_sym) in rule.0.iter().enumerate() {
                start_sym_idx += rule_sym*(b_symbol_set.length as SymbolIdx).pow((rule.0.len()-rule_sym_idx-1) as u32);
            }
            start_sym_idx *= (b_symbol_set.length as SymbolIdx).pow((i) as u32);

            let mut end_sym_idx = 0;
            for (rule_sym_idx, rule_sym) in rule.1.iter().enumerate() {
                end_sym_idx += rule_sym*(b_symbol_set.length as SymbolIdx).pow((rule.1.len()-rule_sym_idx-1) as u32);
            }
            end_sym_idx *= (b_symbol_set.length as SymbolIdx).pow(i as u32);
            new_rules.push((vec![start_sym_idx],vec![end_sym_idx]));
        }
        //Vertical (normal symbol length) rules
        //i is horizontal index selected
        //Represents the fixed column we're doing business with
        
        
        for i in 0..k {
            //LHS and RHS respectively
            vert_starts = vec![vec![0;rule.0.len()]];
            vert_ends = vec![vec![0;rule.1.len()]];
            //j is vertical index selected
            for j in 0..k {
                let cur_vert_rules = vert_starts.len();
                let pow_num = (b_symbol_set.length as SymbolIdx).pow(j as u32);
                
                //If we're looking at the fixed row
                if i == j {
                    for start_idx in 0..cur_vert_rules {
                        for vert_idx in 0..vert_starts[start_idx].len() {
                            vert_starts[start_idx][vert_idx] += rule.0[vert_idx]*pow_num;
                        }
                        for vert_idx in 0..vert_ends[start_idx].len() {
                            vert_ends[start_idx][vert_idx] += rule.1[vert_idx]*pow_num;
                        }
                    }
                } else {
                    for start_idx in 0..cur_vert_rules {
                        for k in 1..symbol_num {
                            let mut new_vert_start = vert_starts[start_idx].clone();
                            let mut new_vert_end = vert_ends[start_idx].clone();
                            for l in 0..k {
                                if (k >> l) % 2 == 1 {
                                    new_vert_start[l] += pow_num;
                                    new_vert_end[l] += pow_num;
                                }
                            }
                            
                            vert_starts.push(new_vert_start);
                            vert_ends.push(new_vert_end);
                        }
                    }
                }
                
            }
            for i in 0..vert_starts.len() {
                new_rules.push((vert_starts[i].clone(),vert_ends[i].clone()));
            }
        }
    }
    let by_k_symbol_set = SymbolSet {
        length : 2_u32.pow(k as u32) as usize,
        representations : vec!["000".to_owned(),"001".to_owned(),"010".to_owned(),"011".to_owned(),"100".to_owned(),"101".to_owned(),"110".to_owned(),"111".to_owned()] //whoops! lol
    };
    
    let ruleset = Ruleset::new(new_rules,by_k_symbol_set.clone());
    
    let root_dfa = DFA::load("default1dpeg").unwrap();

    let mut trans_table = vec![vec![1,2,2+16,2+16*2,2+16*2, 10,2,10],vec![1,3,3+16,3+16*2,3+16*2, 10,3,10]];
    for point in 0..=2 {
        let identical_indices = vec![vec![1,6],vec![2],vec![4,3]];
        
        
        for state in 2..root_dfa.state_transitions.len() {
            let mut new_vec = vec![10;8];
            for thing in &identical_indices[point] {
                new_vec[*thing] = root_dfa.state_transitions[state][1] + point * 16;
            }
            new_vec[0] = root_dfa.state_transitions[state][0] + point * 16;
            trans_table.push(new_vec);
        }
        
    }
    let mut new_accepting = root_dfa.accepting_states.clone();
    for i in root_dfa.accepting_states {
        new_accepting.insert(i + 16);
        new_accepting.insert(i + 32);
    }  

    
    
    let goal_dfa = DFA {
        starting_state : 0,
        state_transitions : trans_table,
        accepting_states : new_accepting,
        symbol_set : by_k_symbol_set.clone()
    };
    goal_dfa.jflap_save("experimental3xk");
    
    
    SRSTranslator::new(ruleset,goal_dfa)
}

//Testing subset method :((


fn main() {
    println!("default 1d");
    let mut translator = build_default1dpeg();
    assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("default1dpeg").unwrap(), "Default 1d failed");
    println!("three rule 1d");
    let mut translator = build_threerule1dpeg();
    assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("threerule1dpeg").unwrap(), "three rule 1d failed");
    println!("default solver");
    let mut translator: SRSTranslator = build_defaultsolver();
    assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("defaultsolver").unwrap(), "Default solver failed");
    //println!("three rule solver");
    //let mut translator = build_threerulesolver();
    //assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("threerulesolver").unwrap(), "Three rule solver failed");
    println!("flip");
    let mut translator: SRSTranslator = build_flip();
    assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("flip").unwrap(), "Flip failed");
    //println!("flipx3");
    //let mut translator: SRSTranslator = build_flipx3();
    //assert!(translator.dfa_with_sig_set_minkid(5) == DFA::load("flipx3").unwrap(), "Flipx3 failed");
    println!("hope and prayer");
    let mut translator = build_2xnswap();
    assert!(translator.dfa_with_sig_set_minkid(12) == DFA::load("2xnswap").unwrap(), "2xnswap failed");
} 