mod ptr;
mod step;
mod store;
mod tag;

use std::collections::{BTreeMap, HashMap};

use self::{ptr::Ptr, ptr::PtrVal, store::Store, tag::Tag};

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
    Set(MetaPtr<'a>, Tag, PtrVal),
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

pub struct LEM<'a> {
    input: [&'a str; 3],
    output: [&'a str; 3],
    lem_op: LEMOP<'a>,
}

impl<'a> LEM<'a> {
    pub fn check(&self) {
        for s in self.input.iter() {
            assert!(
                !self.output.contains(&s),
                "Input and output must be disjoint"
            )
        }
        // TODO: assert that the input and output pointers are all different
        // TODO: assert that all tag field elements are in range
        // TODO: assert that used pointers have been previously defined
        // TODO: assert that input pointers aren't overwritten (including the input)
        // TODO: assert that all input pointers are used
        // TODO: assert that all output pointers are defined
    }

    // pub fn compile should generate the circuit

    pub fn run(
        &self,
        i: [Ptr; 3],
        store: &mut Store,
    ) -> Result<([Ptr; 3], HashMap<&'a str, Ptr>), String> {
        // key/val pairs on this map should never be overwritten
        let mut map: HashMap<&'a str, Ptr> = HashMap::default();
        map.insert(self.input[0], i[0]);
        if map.insert(self.input[1], i[1]).is_some() {
            return Err(format!("{} already defined. Malformed LEM", self.input[1]));
        }
        if map.insert(self.input[2], i[2]).is_some() {
            return Err(format!("{} already defined. Malformed LEM", self.input[2]));
        }
        let mut stack = vec![&self.lem_op];
        while let Some(op) = stack.pop() {
            match op {
                LEMOP::Set(tgt, tag, val) => {
                    let tgt_ptr = Ptr {
                        tag: *tag,
                        val: *val,
                    };
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined. Malformed LEM", tgt.name()));
                    }
                }
                LEMOP::Copy(tgt, src) => {
                    let src_ptr = map.get(src.name()).unwrap();
                    if map.insert(tgt.name(), src_ptr.clone()).is_some() {
                        return Err(format!("{} already defined. Malformed LEM", tgt.name()));
                    }
                }
                LEMOP::Hash2Ptrs(tgt, tag, src) => {
                    let src_ptr1 = map.get(src[0].name()).unwrap();
                    let src_ptr2 = map.get(src[1].name()).unwrap();
                    let (idx, _) = store.ptr2_store.insert_full((*src_ptr1, *src_ptr2));
                    let tgt_ptr = Ptr {
                        tag: *tag,
                        val: PtrVal::Idx(idx),
                    };
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined. Malformed LEM", tgt.name()));
                    }
                }
                LEMOP::Hash3Ptrs(tgt, tag, src) => {
                    let src_ptr1 = map.get(src[0].name()).unwrap();
                    let src_ptr2 = map.get(src[1].name()).unwrap();
                    let src_ptr3 = map.get(src[2].name()).unwrap();
                    let (idx, _) = store
                        .ptr3_store
                        .insert_full((*src_ptr1, *src_ptr2, *src_ptr3));
                    let tgt_ptr = Ptr {
                        tag: *tag,
                        val: PtrVal::Idx(idx),
                    };
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined. Malformed LEM", tgt.name()));
                    }
                }
                LEMOP::Hash4Ptrs(tgt, tag, src) => {
                    let src_ptr1 = map.get(src[0].name()).unwrap();
                    let src_ptr2 = map.get(src[1].name()).unwrap();
                    let src_ptr3 = map.get(src[2].name()).unwrap();
                    let src_ptr4 = map.get(src[3].name()).unwrap();
                    let (idx, _) = store
                        .ptr4_store
                        .insert_full((*src_ptr1, *src_ptr2, *src_ptr3, *src_ptr4));
                    let tgt_ptr = Ptr {
                        tag: *tag,
                        val: PtrVal::Idx(idx),
                    };
                    if map.insert(tgt.name(), tgt_ptr).is_some() {
                        return Err(format!("{} already defined. Malformed LEM", tgt.name()));
                    }
                }
                LEMOP::Unhash2Ptrs(tgts, src) => {
                    let src_ptr = map.get(src.name()).unwrap();
                    if let PtrVal::Idx(idx) = src_ptr.val {
                        let (a, b) = store.ptr2_store.get_index(idx).unwrap();
                        if map.insert(tgts[0].name(), *a).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[0].name()
                            ));
                        }
                        if map.insert(tgts[1].name(), *b).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[1].name()
                            ));
                        }
                    } else {
                        return Err(format!(
                            "{} is an invalid pointer to unhash. Malformed LEM",
                            src.name()
                        ));
                    }
                }
                LEMOP::Unhash3Ptrs(tgts, src) => {
                    let src_ptr = map.get(src.name()).unwrap();
                    if let PtrVal::Idx(idx) = src_ptr.val {
                        let (a, b, c) = store.ptr3_store.get_index(idx).unwrap();
                        if map.insert(tgts[0].name(), *a).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[0].name()
                            ));
                        }
                        if map.insert(tgts[1].name(), *b).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[1].name()
                            ));
                        }
                        if map.insert(tgts[2].name(), *c).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[2].name()
                            ));
                        }
                    } else {
                        return Err(format!(
                            "{} is an invalid pointer to unhash. Malformed LEM",
                            src.name()
                        ));
                    }
                }
                LEMOP::Unhash4Ptrs(tgts, src) => {
                    let src_ptr = map.get(src.name()).unwrap();
                    if let PtrVal::Idx(idx) = src_ptr.val {
                        let (a, b, c, d) = store.ptr4_store.get_index(idx).unwrap();
                        if map.insert(tgts[0].name(), *a).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[0].name()
                            ));
                        }
                        if map.insert(tgts[1].name(), *b).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[1].name()
                            ));
                        }
                        if map.insert(tgts[2].name(), *c).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[2].name()
                            ));
                        }
                        if map.insert(tgts[3].name(), *d).is_some() {
                            return Err(format!(
                                "{} already defined. Malformed LEM",
                                tgts[3].name()
                            ));
                        }
                    } else {
                        return Err(format!(
                            "{} is an invalid pointer to unhash. Malformed LEM",
                            src.name()
                        ));
                    }
                }
                LEMOP::MatchTag(ptr, cases, def) => {
                    let ptr_match = map.get(ptr.name()).unwrap();
                    match cases.get(&ptr_match.tag) {
                        Some(op) => stack.push(op),
                        None => stack.push(def),
                    }
                }
                LEMOP::Seq(ops) => stack.extend(ops.iter().rev()),
                LEMOP::Err(s) => return Err(s.to_string()), // this should use the error continuation
            }
        }
        Ok((
            [
                *map.get(self.output[0]).unwrap(),
                *map.get(self.output[1]).unwrap(),
                *map.get(self.output[2]).unwrap(),
            ],
            map,
        ))
    }
}
