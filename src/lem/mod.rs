mod pointers;
mod step;
mod store;
mod symbol;
mod tag;

use std::collections::{BTreeMap, HashMap};

use crate::field::LurkField;

use self::{pointers::LurkData, store::Store, tag::Tag};

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
/// ### Data semantics
///
/// A LEM describes how to handle pointers with "meta pointers", with are
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
/// all relevant data lives on a `HashMap` that's also a product of the
/// interpreted LEM.
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
/// 3. Output assignment completeness: at the end of every step we want all the
/// output labels to be bound to some pointer otherwise we wouldn't know how to
/// proceed on the next step;
///
/// 4. Non-duplicated output labels: property 3 forces us have a pointer bound
/// to every output label. If some output label is duplicated, we would fatally
/// break property 1;
///
/// 5. Disjoint input and output labels: if an input label is also present in
/// the output, satisfying property 3 would break property 1 because such label
/// would be bound twice;
///
/// 6. Assign first, use later: this prevents obvious "x not found" errors at
/// interpretation time.
pub struct LEM<'a> {
    input: [&'a str; 3],
    output: [&'a str; 3],
    lem_op: LEMOP<'a>,
}

#[derive(PartialEq, Clone, Copy)]
pub struct MetaPtr<'a>(&'a str);

impl<'a> MetaPtr<'a> {
    #[inline]
    pub fn name(self) -> &'a str {
        self.0
    }
}

#[derive(Clone)]
pub enum LEMOP<'a> {
    Set(MetaPtr<'a>, Tag),
    Copy(MetaPtr<'a>, MetaPtr<'a>),
    Hash2Ptrs(MetaPtr<'a>, Tag, [MetaPtr<'a>; 2]),
    Hash3Ptrs(MetaPtr<'a>, Tag, [MetaPtr<'a>; 3]),
    Hash4Ptrs(MetaPtr<'a>, Tag, [MetaPtr<'a>; 4]),
    Unhash2Ptrs([MetaPtr<'a>; 2], MetaPtr<'a>),
    Unhash3Ptrs([MetaPtr<'a>; 3], MetaPtr<'a>),
    Unhash4Ptrs([MetaPtr<'a>; 4], MetaPtr<'a>),
    MatchTag(MetaPtr<'a>, BTreeMap<Tag, LEMOP<'a>>, Box<LEMOP<'a>>),
    Err(&'a str),
    Seq(Vec<LEMOP<'a>>),
}

impl<'a> LEMOP<'a> {
    pub fn assert_tag_eq(ptr: MetaPtr<'a>, tag: Tag, ff: LEMOP<'a>, tt: LEMOP<'a>) -> LEMOP<'a> {
        let mut map = BTreeMap::new();
        map.insert(tag, tt);
        LEMOP::MatchTag(ptr, map, Box::new(ff))
    }

    pub fn assert_tag_or(
        ptr: MetaPtr<'a>,
        val1: Tag,
        val2: Tag,
        ff: LEMOP<'a>,
        tt: LEMOP<'a>,
    ) -> LEMOP<'a> {
        let mut map = BTreeMap::new();
        map.insert(val1, tt.clone());
        map.insert(val2, tt);
        LEMOP::MatchTag(ptr, map, Box::new(ff))
    }

    pub fn assert_list(ptr: MetaPtr<'a>, ff: LEMOP<'a>, tt: LEMOP<'a>) -> LEMOP<'a> {
        Self::assert_tag_or(ptr, Tag::Cons, Tag::Nil, ff, tt)
    }

    pub fn mk_cons(o: &'a str, i: [MetaPtr<'a>; 2]) -> LEMOP<'a> {
        LEMOP::Hash2Ptrs(MetaPtr(o), Tag::Cons, i)
    }

    pub fn mk_strcons(o: &'a str, i: [MetaPtr<'a>; 2]) -> LEMOP<'a> {
        Self::assert_tag_eq(
            i[0],
            Tag::Char,
            LEMOP::Err("strcons requires a char as the first argument"),
            Self::assert_tag_eq(
                i[1],
                Tag::Str,
                LEMOP::Err("strcons requires a str as the second argument"),
                LEMOP::Hash2Ptrs(MetaPtr(o), Tag::Str, i),
            ),
        )
    }

    pub fn mk_fun(o: &'a str, i: [MetaPtr<'a>; 3]) -> LEMOP<'a> {
        Self::assert_list(
            i[0],
            LEMOP::Err("The arguments must be a list"),
            Self::assert_list(
                i[2],
                LEMOP::Err("The closed env must be a list"),
                LEMOP::Hash3Ptrs(MetaPtr(o), Tag::Fun, i),
            ),
        )
    }

    pub fn mk_match_tag(i: MetaPtr<'a>, cases: Vec<(Tag, LEMOP<'a>)>, def: LEMOP<'a>) -> LEMOP<'a> {
        let mut match_map = BTreeMap::default();
        for (f, op) in cases.iter() {
            match_map.insert(*f, op.clone());
        }
        LEMOP::MatchTag(i, match_map, Box::new(def))
    }
}

impl<'a> LEM<'a> {
    pub fn check(&self) {
        for s in self.input.iter() {
            assert!(
                !self.output.contains(&s),
                "Input and output must be disjoint"
            )
        }
        // TODO
    }

    // pub fn compile should generate the circuit

    pub fn run<F: LurkField + std::hash::Hash>(
        &self,
        i: [LurkData<F>; 3],
        store: &mut Store<F>,
    ) -> Result<([LurkData<F>; 3], HashMap<&'a str, LurkData<F>>), String> {
        // key/val pairs on this map should never be overwritten
        let mut map: HashMap<&'a str, LurkData<F>> = HashMap::default();
        map.insert(self.input[0], i[0]);
        if map.insert(self.input[1], i[1]).is_some() {
            return Err(format!("{} already defined", self.input[1]));
        }
        if map.insert(self.input[2], i[2]).is_some() {
            return Err(format!("{} already defined", self.input[2]));
        }
        let mut stack = vec![&self.lem_op];
        while let Some(op) = stack.pop() {
            match op {
                LEMOP::Set(tgt, tag) => {
                    let tgt_ptr = LurkData::Tag(*tag);
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined", tgt.name()));
                    }
                }
                LEMOP::Copy(tgt, src) => {
                    let Some(src_ptr) = map.get(src.name()) else {
                        return Err(format!("{} not defined", src.name()))
                    };
                    if map.insert(tgt.name(), *src_ptr).is_some() {
                        return Err(format!("{} already defined", tgt.name()));
                    }
                }
                LEMOP::Hash2Ptrs(tgt, tag, src) => {
                    let Some(src_ptr1) = map.get(src[0].name()) else {
                        return Err(format!("{} not defined", src[0].name()))
                    };
                    let Some(src_ptr2) = map.get(src[1].name()) else {
                        return Err(format!("{} not defined", src[1].name()))
                    };
                    let (idx, _) = store.data2.insert_full((*src_ptr1, *src_ptr2));
                    let tgt_ptr = LurkData::Ptr(*tag, idx);
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined", tgt.name()));
                    }
                }
                LEMOP::Hash3Ptrs(tgt, tag, src) => {
                    let Some(src_ptr1) = map.get(src[0].name()) else {
                        return Err(format!("{} not defined", src[0].name()))
                    };
                    let Some(src_ptr2) = map.get(src[1].name()) else {
                        return Err(format!("{} not defined", src[1].name()))
                    };
                    let Some(src_ptr3) = map.get(src[2].name()) else {
                        return Err(format!("{} not defined", src[2].name()))
                    };
                    let (idx, _) = store.data3.insert_full((*src_ptr1, *src_ptr2, *src_ptr3));
                    let tgt_ptr = LurkData::Ptr(*tag, idx);
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined", tgt.name()));
                    }
                }
                LEMOP::Hash4Ptrs(tgt, tag, src) => {
                    let Some(src_ptr1) = map.get(src[0].name()) else {
                        return Err(format!("{} not defined", src[0].name()))
                    };
                    let Some(src_ptr2) = map.get(src[1].name()) else {
                        return Err(format!("{} not defined", src[1].name()))
                    };
                    let Some(src_ptr3) = map.get(src[2].name()) else {
                        return Err(format!("{} not defined", src[2].name()))
                    };
                    let Some(src_ptr4) = map.get(src[3].name()) else {
                        return Err(format!("{} not defined", src[3].name()))
                    };
                    let (idx, _) = store
                        .data4
                        .insert_full((*src_ptr1, *src_ptr2, *src_ptr3, *src_ptr4));
                    let tgt_ptr = LurkData::Ptr(*tag, idx);
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined", tgt.name()));
                    }
                }
                LEMOP::Unhash2Ptrs(tgts, src) => {
                    let Some(src_ptr) = map.get(src.name()) else {
                        return Err(format!("{} not defined", src.name()))
                    };
                    let LurkData::Ptr(_, idx) = src_ptr else {
                        return Err(format!(
                            "{} is bound to a null pointer and can't be unhashed",
                            src.name()
                        ));
                    };
                    let Some((a, b)) = store.data2.get_index(*idx) else {
                        return Err(format!("{} isn't bound to a 2-hashed pointer", src.name()))
                    };
                    if map.insert(tgts[0].name(), *a).is_some() {
                        return Err(format!("{} already defined", tgts[0].name()));
                    }
                    if map.insert(tgts[1].name(), *b).is_some() {
                        return Err(format!("{} already defined", tgts[1].name()));
                    }
                }
                LEMOP::Unhash3Ptrs(tgts, src) => {
                    let Some(src_ptr) = map.get(src.name()) else {
                        return Err(format!("{} not defined", src.name()))
                    };
                    let LurkData::Ptr(_, idx) = src_ptr else {
                        return Err(format!(
                            "{} is bound to a null pointer and can't be unhashed",
                            src.name()
                        ));
                    };
                    let Some((a, b, c)) = store.data3.get_index(*idx) else {
                        return Err(format!("{} isn't bound to a 3-hashed pointer", src.name()))
                    };
                    if map.insert(tgts[0].name(), *a).is_some() {
                        return Err(format!("{} already defined", tgts[0].name()));
                    }
                    if map.insert(tgts[1].name(), *b).is_some() {
                        return Err(format!("{} already defined", tgts[1].name()));
                    }
                    if map.insert(tgts[2].name(), *c).is_some() {
                        return Err(format!("{} already defined", tgts[2].name()));
                    }
                }
                LEMOP::Unhash4Ptrs(tgts, src) => {
                    let Some(src_ptr) = map.get(src.name()) else {
                        return Err(format!("{} not defined", src.name()))
                    };
                    let LurkData::Ptr(_, idx) = src_ptr else {
                        return Err(format!(
                            "{} is bound to a null pointer and can't be unhashed",
                            src.name()
                        ));
                    };
                    let Some((a, b, c, d)) = store.data4.get_index(*idx) else {
                        return Err(format!("{} isn't bound to a 4-hashed pointer", src.name()))
                    };
                    if map.insert(tgts[0].name(), *a).is_some() {
                        return Err(format!("{} already defined", tgts[0].name()));
                    }
                    if map.insert(tgts[1].name(), *b).is_some() {
                        return Err(format!("{} already defined", tgts[1].name()));
                    }
                    if map.insert(tgts[2].name(), *c).is_some() {
                        return Err(format!("{} already defined", tgts[2].name()));
                    }
                    if map.insert(tgts[3].name(), *d).is_some() {
                        return Err(format!("{} already defined", tgts[3].name()));
                    }
                }
                LEMOP::MatchTag(ptr, cases, def) => {
                    let Some(LurkData::Ptr(tag, _)) = map.get(ptr.name()) else {
                        return Err(format!("{} not defined", ptr.name()))
                    };
                    match cases.get(tag) {
                        Some(op) => stack.push(op),
                        None => stack.push(def),
                    }
                }
                LEMOP::Seq(ops) => stack.extend(ops.iter().rev()),
                LEMOP::Err(s) => return Err(s.to_string()), // this should use the error continuation
            }
        }
        let Some(out1) = map.get(self.output[0]) else {
            return Err(format!("Output {} not defined", self.output[0]))
        };
        let Some(out2) = map.get(self.output[1]) else {
            return Err(format!("Output {} not defined", self.output[1]))
        };
        let Some(out3) = map.get(self.output[2]) else {
            return Err(format!("Output {} not defined", self.output[2]))
        };
        Ok(([*out1, *out2, *out3], map))
    }
}
