mod constrainer;
mod eval;
mod interpreter;
mod pointers;
mod store;
mod symbol;
mod tag;

use crate::field::LurkField;
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

use self::{pointers::Ptr, store::Store, tag::Tag};

use dashmap::DashMap;

/// ## Lurk Evaluation Model (LEM)
///
/// A LEM is a description of Lurk's evaluation algorithm, encoded as data. In
/// other words, it's a meta-representation of Lurk's step function.
///
/// The motivation behind LEM is the fact that hand-writing the circuit is a
/// fragile process that hinders experimentation and safety. Thus we would like
/// to bootstrap the circuit automatically, given a higher level description of
/// the step function.
///
/// LEM also allows the `Store` API to be completely abstracted away from the
/// responsibilities of LEM authors. Indeed, we want the implementation details
/// of the `Store` not to be important at LEM definition time.
///
/// ### Data semantics
///
/// A LEM describes how to handle pointers with "meta pointers", which are
/// basically named references. Instead of saying `let foo ...` in Rust, we
/// use a `MetaPtr("foo")` in LEM.
///
/// The actual algorithm is encoded with a LEM operation (`LEMOP`). It's worth
/// noting that one of the LEM operators is in fact a vector of operators, which
/// allows imperative expressiveness.
///
/// ### Interpretation
///
/// Running a LEM is done via interpretation, which might be a bit slower than
/// calling Rust functions directly. But it also has its advantages:
///
/// 1. The logic to collect data during execution can be factored out from the
/// definition of the step function. This process is needed in order to evidence
/// the inputs for the circuit at proving time;
///
/// 2. Actually, such logic to collect data is a natural consequence of the fact
/// that we're on a higher level of abstraction. Relevant data is not simply
/// stored on rust variables that die after the function ends. On the contrary,
/// all relevant data lives on `HashMap`s that are also a product of the
/// interpreted LEM.
///
/// ### Constraining
///
/// This is the process of creating the circuit, which we want to be done
/// automatically for whoever creates a LEM. Each `LEMOP` has to be precisely
/// constrained in such a way that the resulting circuits accepts a witness iff
/// it was generated by a valid interpretation of the LEM at play.
///
/// ### Static checks of correctness
///
/// Since a LEM is an algorithm encoded as data, we can perform static checks of
/// correctness as some form of (automated) formal verification. Here are some
/// (WIP) properties we want a LEM to have before we can adopt it as a proper
/// Lurk step function:
///
/// 1. Static single assignments: overwriting meta pointers would erase relevant
/// data needed to feed the circuit at proving time. We don't want to lose any
/// piece of information that the prover might know;
///
/// 2. Non-duplicated input labels: right at the start of interpretation, the
/// input labels are bound to the actual pointers that represent the expression,
/// environment and continuation. If some label is repeated, it will fatally
/// break property 1;
///
/// 3. One return per LEM path: a LEM must always specify an output regardless
/// of the logical path it takes at interpretation time, otherwise there would
/// be a chance of the next step starting with an unknown input. Also, a LEM
/// should not specify more than an output per logical path because it would
/// risk setting conflicting constraints for the output;
///
/// 4. Assign first, use later: this prevents obvious errors such as "x not
/// defined" during interpretation or "x not allocated" during constraining.
pub struct LEM {
    input: [String; 3],
    lem_op: LEMOP,
}

/// Named references to be bound to `Ptr`s.
#[derive(PartialEq, Clone, Eq, Hash)]
pub struct MetaPtr(String);

impl MetaPtr {
    #[inline]
    pub fn name(&self) -> &String {
        &self.0
    }

    pub fn get_ptr<F: LurkField>(&self, ptrs: &HashMap<String, Ptr<F>>) -> Result<Ptr<F>> {
        match ptrs.get(&self.0) {
            Some(ptr) => Ok(*ptr),
            None => Err(anyhow!("Meta pointer {} not defined", self.0)),
        }
    }
}

/// The basic building blocks of LEMs.
#[derive(Clone)]
pub enum LEMOP {
    /// `MkNull(x, t)` binds `x` to a `Ptr::Leaf(t, F::zero())`
    MkNull(MetaPtr, Tag),
    /// `Hash2Ptrs(x, t, is)` binds `x` to a `Ptr` with tag `t` and 2 children `is`
    Hash2Ptrs(MetaPtr, Tag, [MetaPtr; 2]),
    /// `Hash3Ptrs(x, t, is)` binds `x` to a `Ptr` with tag `t` and 3 children `is`
    Hash3Ptrs(MetaPtr, Tag, [MetaPtr; 3]),
    /// `Hash4Ptrs(x, t, is)` binds `x` to a `Ptr` with tag `t` and 4 children `is`
    Hash4Ptrs(MetaPtr, Tag, [MetaPtr; 4]),
    /// `Unhash2Ptrs([a, b], x)` binds `a` and `b` to the 2 children of `x`
    Unhash2Ptrs([MetaPtr; 2], MetaPtr),
    /// `Unhash3Ptrs([a, b, c], x)` binds `a` and `b` to the 3 children of `x`
    Unhash3Ptrs([MetaPtr; 3], MetaPtr),
    /// `Unhash4Ptrs([a, b, c, d], x)` binds `a` and `b` to the 4 children of `x`
    Unhash4Ptrs([MetaPtr; 4], MetaPtr),
    /// `Hide(x, s, p)` binds `x` to a (comm) `Ptr` resulting from hiding the
    /// payload `p` with (num) secret `s`
    Hide(MetaPtr, MetaPtr, MetaPtr),
    /// `Open(s, p, h)` binds `s` and `p` to the secret and payload (respectively)
    /// of the commitment that resulted on (num or comm) `h`
    Open(MetaPtr, MetaPtr, MetaPtr),
    /// `MatchTag(x, cases)` performs a match on the tag of `x`, considering only
    /// the appropriate `LEMOP` among the ones provided in `cases`
    MatchTag(MetaPtr, HashMap<Tag, LEMOP>),
    /// `MatchSymPath(x, cases, def)` checks whether `x` matches some symbol among
    /// the ones provided in `cases`. If so, run the corresponding `LEMOP`. Run
    /// The default `def` `LEMOP` otherwise
    MatchSymPath(MetaPtr, HashMap<Vec<String>, LEMOP>, Box<LEMOP>),
    /// `Seq(ops)` executes each `op: LEMOP` in `ops` sequentially
    Seq(Vec<LEMOP>),
    /// `SetReturn([a, b, c])` sets the output as `[a, b, c]`
    SetReturn([MetaPtr; 3]),
}

impl LEMOP {
    /// Performs the static checks of correctness described in `LEM`.
    ///
    /// Note: this function is not supposed to be called manually. It's used by
    /// `LEM::new`, which is the API that should be used directly.
    pub fn check(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    /// Removes conflicting names in parallel logical LEM paths. While these
    /// conflicting names shouldn't be an issue for interpretation, they are
    /// problematic when we want to generate the constraints for the LEM, since
    /// conflicting names would cause different allocations to be bound the same
    /// name.
    ///
    /// Note: this function is not supposed to be called manually. It's used by
    /// `LEM::new`, which is the API that should be used directly.
    pub fn deconflict(
        &self,
        path: String,
        dmap: &DashMap<String, String, ahash::RandomState>, // name -> path/name
    ) -> Result<Self> {
        match self {
            Self::MkNull(ptr, tag) => {
                let new_name = format!("{}.{}", path, ptr.name());
                if dmap.insert(ptr.name().clone(), new_name.clone()).is_some() {
                    bail!("{} already defined", ptr.name());
                };
                Ok(Self::MkNull(MetaPtr(new_name), *tag))
            }
            Self::Hash2Ptrs(tgt, tag, src) => {
                let Some(src0_path) = dmap.get(src[0].name()) else {
                    bail!("{} not defined", src[0].name());
                };
                let Some(src1_path) = dmap.get(src[1].name()) else {
                    bail!("{} not defined", src[1].name());
                };
                let new_name = format!("{}.{}", path, tgt.name());
                if dmap.insert(tgt.name().clone(), new_name.clone()).is_some() {
                    bail!("{} already defined", tgt.name());
                };
                Ok(Self::Hash2Ptrs(
                    MetaPtr(new_name),
                    *tag,
                    [MetaPtr(src0_path.clone()), MetaPtr(src1_path.clone())],
                ))
            }
            Self::Hash3Ptrs(tgt, tag, src) => {
                let Some(src0_path) = dmap.get(src[0].name()) else {
                    bail!("{} not defined", src[0].name());
                };
                let Some(src1_path) = dmap.get(src[1].name()) else {
                    bail!("{} not defined", src[1].name());
                };
                let Some(src2_path) = dmap.get(src[2].name()) else {
                    bail!("{} not defined", src[2].name());
                };
                let new_name = format!("{}.{}", path, tgt.name());
                if dmap.insert(tgt.name().clone(), new_name.clone()).is_some() {
                    bail!("{} already defined", tgt.name());
                };
                Ok(Self::Hash3Ptrs(
                    MetaPtr(new_name),
                    *tag,
                    [
                        MetaPtr(src0_path.clone()),
                        MetaPtr(src1_path.clone()),
                        MetaPtr(src2_path.clone()),
                    ],
                ))
            }
            Self::Hash4Ptrs(tgt, tag, src) => {
                let Some(src0_path) = dmap.get(src[0].name()) else {
                    bail!("{} not defined", src[0].name());
                };
                let Some(src1_path) = dmap.get(src[1].name()) else {
                    bail!("{} not defined", src[1].name());
                };
                let Some(src2_path) = dmap.get(src[2].name()) else {
                    bail!("{} not defined", src[2].name());
                };
                let Some(src3_path) = dmap.get(src[3].name()) else {
                    bail!("{} not defined", src[3].name());
                };
                let new_name = format!("{}.{}", path, tgt.name());
                if dmap.insert(tgt.name().clone(), new_name.clone()).is_some() {
                    bail!("{} already defined", tgt.name());
                };
                Ok(Self::Hash4Ptrs(
                    MetaPtr(new_name),
                    *tag,
                    [
                        MetaPtr(src0_path.clone()),
                        MetaPtr(src1_path.clone()),
                        MetaPtr(src2_path.clone()),
                        MetaPtr(src3_path.clone()),
                    ],
                ))
            }
            LEMOP::MatchTag(ptr, cases) => {
                let Some(ptr_path) = dmap.get(ptr.name()) else {
                    bail!("{} not defined", ptr.name());
                };
                let mut new_cases = vec![];
                for (tag, case) in cases {
                    // each case needs it's own clone of `dmap`
                    let new_case = case.deconflict(format!("{}.{}", &path, &tag), &dmap.clone())?;
                    new_cases.push((*tag, new_case));
                }
                Ok(LEMOP::MatchTag(
                    MetaPtr(ptr_path.clone()),
                    HashMap::from_iter(new_cases),
                ))
            }
            LEMOP::Seq(ops) => {
                let mut new_ops = vec![];
                for op in ops {
                    new_ops.push(op.deconflict(path.clone(), dmap)?);
                }
                Ok(LEMOP::Seq(new_ops))
            }
            LEMOP::SetReturn(o) => {
                let Some(o0) = dmap.get(o[0].name()) else {
                    bail!("{} not defined", o[0].name());
                };
                let Some(o1) = dmap.get(o[1].name()) else {
                    bail!("{} not defined", o[1].name());
                };
                let Some(o2) = dmap.get(o[2].name()) else {
                    bail!("{} not defined", o[2].name());
                };
                Ok(LEMOP::SetReturn([
                    MetaPtr(o0.clone()),
                    MetaPtr(o1.clone()),
                    MetaPtr(o2.clone()),
                ]))
            }
            _ => todo!(),
        }
    }

    /// Intern all symbol paths that are matched on `MatchSymPath`s
    pub fn intern_matched_sym_paths<F: LurkField>(&self, store: &mut Store<F>) {
        let mut stack = vec![self];
        while let Some(op) = stack.pop() {
            match op {
                Self::MatchSymPath(_, cases, def) => {
                    for (path, op) in cases {
                        store.intern_symbol_path(path);
                        stack.push(op);
                    }
                    stack.push(def);
                }
                Self::MatchTag(_, cases) => cases.values().for_each(|op| stack.push(op)),
                Self::Seq(ops) => stack.extend(ops),
                // It's safer to be exaustive here and avoid missing new LEMOPs
                Self::MkNull(..)
                | Self::Hash2Ptrs(..)
                | Self::Hash3Ptrs(..)
                | Self::Hash4Ptrs(..)
                | Self::Unhash2Ptrs(..)
                | Self::Unhash3Ptrs(..)
                | Self::Unhash4Ptrs(..)
                | Self::Hide(..)
                | Self::Open(..)
                | Self::SetReturn(..) => (),
            }
        }
    }
}

/// A `Witness` carries the data that results from interpreting LEM. That is,
/// it contains all the assignments resulting from running one iteration.
#[derive(Clone)]
#[allow(dead_code)]
pub struct Witness<F: LurkField> {
    input: [Ptr<F>; 3],
    output: [Ptr<F>; 3],
    ptrs: HashMap<String, Ptr<F>>,
}

impl LEM {
    /// Instantiates a `LEM` with the appropriate checks and transformations
    /// to make sure that interpretation and constraining will be smooth.
    pub fn new(input: [&str; 3], lem_op: LEMOP) -> Result<LEM> {
        lem_op.check()?;
        let dmap = DashMap::from_iter(input.map(|i| (i.to_string(), i.to_string())));
        Ok(LEM {
            input: input.map(|i| i.to_string()),
            lem_op: lem_op.deconflict(String::new(), &dmap)?,
        })
    }
}

mod shortcuts {
    use super::*;

    #[allow(dead_code)]
    #[inline]
    pub(crate) fn mptr(name: &str) -> MetaPtr {
        MetaPtr(name.to_string())
    }

    #[allow(dead_code)]
    #[inline]
    pub(crate) fn match_tag(i: MetaPtr, cases: Vec<(Tag, LEMOP)>) -> LEMOP {
        LEMOP::MatchTag(i, HashMap::from_iter(cases))
    }
}

#[cfg(test)]
mod tests {
    use super::{store::Store, *};
    use crate::lem::{pointers::Ptr, tag::Tag};
    use bellperson::util_cs::test_cs::TestConstraintSystem;
    use blstrs::Scalar as Fr;
    use shortcuts::*;

    fn constrain_test_helper(lem: &LEM, store: &mut Store<Fr>, witnesses: &Vec<Witness<Fr>>) {
        for w in witnesses {
            let mut cs = TestConstraintSystem::<Fr>::new();
            lem.constrain(&mut cs, store, w).unwrap();
            assert!(cs.is_satisfied());
        }
    }

    #[test]
    fn accepts_virtual_nested_match_tag() {
        let input = ["expr_in", "env_in", "cont_in"];
        let lem_op = match_tag(
            mptr("expr_in"),
            vec![
                (
                    Tag::Num,
                    LEMOP::Seq(vec![
                        LEMOP::MkNull(mptr("cont_out_terminal"), Tag::Terminal),
                        LEMOP::SetReturn([
                            mptr("expr_in"),
                            mptr("env_in"),
                            mptr("cont_out_terminal"),
                        ]),
                    ]),
                ),
                (
                    Tag::Char,
                    match_tag(
                        // This nested match excercises the need to pass on the information
                        // that we are on a virtual branch, because a constrain will
                        // be created for `cont_out_error` and it will need to be relaxed
                        // by an implication with a false premise
                        mptr("expr_in"),
                        vec![(
                            Tag::Num,
                            LEMOP::Seq(vec![
                                LEMOP::MkNull(mptr("cont_out_error"), Tag::Error),
                                LEMOP::SetReturn([
                                    mptr("expr_in"),
                                    mptr("env_in"),
                                    mptr("cont_out_error"),
                                ]),
                            ]),
                        )],
                    ),
                ),
                (
                    Tag::Sym,
                    match_tag(
                        // This nested match exercises the need to relax `popcount`
                        // because there is no match but it's on a virtual path, so
                        // we don't want to be too restrictive
                        mptr("expr_in"),
                        vec![(
                            Tag::Char,
                            LEMOP::SetReturn([mptr("expr_in"), mptr("env_in"), mptr("cont_in")]),
                        )],
                    ),
                ),
            ],
        );
        let lem = LEM::new(input, lem_op).unwrap();

        let expr = Ptr::num(Fr::from_u64(42));
        let mut store = Store::default();
        let witnesses = lem.eval(expr, &mut store).unwrap();
        constrain_test_helper(&lem, &mut store, &witnesses);
    }

    #[test]
    fn resolves_conflicts_of_clashing_names_in_parallel_branches() {
        let input = ["expr_in", "env_in", "cont_in"];
        let lem_op = match_tag(
            // This match is creating `cont_out_terminal` on two different branches,
            // which, in theory, would cause troubles at allocation time. But we're
            // dealing with that automatically
            mptr("expr_in"),
            vec![
                (
                    Tag::Num,
                    LEMOP::Seq(vec![
                        LEMOP::MkNull(mptr("cont_out_terminal"), Tag::Terminal),
                        LEMOP::SetReturn([
                            mptr("expr_in"),
                            mptr("env_in"),
                            mptr("cont_out_terminal"),
                        ]),
                    ]),
                ),
                (
                    Tag::Char,
                    LEMOP::Seq(vec![
                        LEMOP::MkNull(mptr("cont_out_terminal"), Tag::Terminal),
                        LEMOP::SetReturn([
                            mptr("expr_in"),
                            mptr("env_in"),
                            mptr("cont_out_terminal"),
                        ]),
                    ]),
                ),
            ],
        );
        let lem = LEM::new(input, lem_op).unwrap();

        let expr = Ptr::num(Fr::from_u64(42));
        let mut store = Store::default();
        let witnesses = lem.eval(expr, &mut store).unwrap();
        constrain_test_helper(&lem, &mut store, &witnesses);
    }

    #[test]
    fn test_hash_slots() {
        let input = ["expr_in", "env_in", "cont_in"];
        let lem_op = match_tag(
            mptr("expr_in"),
            vec![
                (
                    Tag::Num,
                    LEMOP::Seq(vec![
                        LEMOP::Hash2Ptrs(mptr("expr_out"), Tag::Cons, [mptr("expr_in"), mptr("expr_in")]),
                        LEMOP::MkNull(mptr("cont_out_terminal"), Tag::Terminal),
                        LEMOP::SetReturn([
                            mptr("expr_out"),
                            mptr("env_in"),
                            mptr("cont_out_terminal"),
                        ]),
                    ]),
                ),
            ],
        );
        let lem = LEM::new(input, lem_op).unwrap();

        let expr = Ptr::num(Fr::from_u64(42));
        let mut store = Store::default();
        let witnesses = lem.eval(expr, &mut store).unwrap();
        constrain_test_helper(&lem, &mut store, &witnesses);
    }
}
