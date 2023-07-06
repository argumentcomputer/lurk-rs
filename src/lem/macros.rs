#[macro_export]
macro_rules! var {
    ($variable:ident) => {
        $crate::lem::Var(stringify!($variable).into())
    };
}

#[macro_export]
macro_rules! vars {
    ($($variable:ident),*) => {
        [
            $($crate::var!($variable)),*
        ]
    };
}

#[macro_export]
macro_rules! op {
    ( let $tgt:ident : $tag:ident ) => {
        $crate::lem::Op::Null($crate::var!($tgt), $crate::lem::Tag::$tag)
    };
    ( let $tgt:ident : $tag:ident = hash2($src1:ident, $src2:ident) ) => {
        $crate::lem::Op::Hash2(
            $crate::var!($tgt),
            $crate::lem::Tag::$tag,
            $crate::vars!($src1, $src2),
        )
    };
    ( let $tgt:ident : $tag:ident = hash3($src1:ident, $src2:ident, $src3:ident) ) => {
        $crate::lem::Op::Hash3(
            $crate::var!($tgt),
            $crate::lem::Tag::$tag,
            $crate::vars!($src1, $src2, $src3),
        )
    };
    ( let $tgt:ident : $tag:ident = hash4($src1:ident, $src2:ident, $src3:ident, $src4:ident) ) => {
        $crate::lem::Op::Hash4(
            $crate::var!($tgt),
            $crate::lem::Tag::$tag,
            $crate::vars!($src1, $src2, $src3, $src4),
        )
    };
    ( let ($tgt1:ident, $tgt2:ident) = unhash2($src:ident) ) => {
        $crate::lem::Op::Unhash2(
            $crate::vars!($tgt1, $tgt2),
            $crate::lem::Var(stringify!($src).into()),
        )
    };
    ( let ($tgt1:ident, $tgt2:ident, $tgt3:ident) = unhash3($src:ident) ) => {
        $crate::lem::Op::Unhash3($crate::vars!($tgt1, $tgt2, $tgt3), $crate::var!($src))
    };
    ( let ($tgt1:ident, $tgt2:ident, $tgt3:ident, $tgt4:ident) = unhash4($src:ident) ) => {
        $crate::lem::Op::Unhash4(
            $crate::vars!($tgt1, $tgt2, $tgt3, $tgt4),
            $crate::var!($src),
        )
    };
    ( let $tgt:ident = hide($sec:ident, $src:ident) ) => {
        $crate::lem::Op::Hide($crate::var!($tgt), $crate::var!($sec), $crate::var!($src))
    };
    ( let ($sec:ident, $src:ident) = open($hash:ident) ) => {
        $crate::lem::Op::Open($crate::var!($sec), $crate::var!($src), $crate::var!($hash))
    };
}

#[macro_export]
macro_rules! ctrl {
    ( match_tag $sii:ident { $( $tag:ident => $case_ops:tt ),* $(,)? } ) => {
        {
            let mut cases = indexmap::IndexMap::new();
            $(
                if cases.insert(
                    $crate::lem::Tag::$tag,
                    $crate::ctrl!( $case_ops ),
                ).is_some() {
                    panic!("Repeated tag on `match_tag`");
                };
            )*
            $crate::lem::Ctrl::MatchTag($crate::var!($sii), cases)
        }
    };
    ( match_symbol $sii:ident { $( $symbol:expr => $case_ops:tt ),* , _ => $def:tt $(,)? } ) => {
        {
            let mut cases = indexmap::IndexMap::new();
            $(
                if cases.insert(
                    $symbol,
                    $crate::ctrl!( $case_ops ),
                ).is_some() {
                    panic!("Repeated path on `match_symbol`");
                };
            )*
            $crate::lem::Ctrl::MatchSymbol($crate::var!($sii), cases, Box::new($crate::ctrl!( $def )))
        }
    };
    ( return ($src1:ident, $src2:ident, $src3:ident) ) => {
        $crate::lem::Ctrl::Return(
            $crate::vars!($src1, $src2, $src3)
        )
    };
    // seq entry point, with a separate bracketing to differentiate
    ({ $($body:tt)+ }) => {
        {
            $crate::ctrl! ( @seq {}, $($body)* )
        }
    };
    // handle the recursion: as we see a statement, we push it to the limbs position in the pattern
    (@seq {$($limbs:expr)*}, let $tgt:ident : $tag:ident ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let $tgt: $tag)
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let $tgt:ident : $tag:ident = hash2($src1:ident, $src2:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let $tgt: $tag = hash2($src1, $src2) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let $tgt:ident : $tag:ident = hash3($src1:ident, $src2:ident, $src3:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let $tgt: $tag = hash3($src1, $src2, $src3) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let $tgt:ident : $tag:ident = hash4($src1:ident, $src2:ident, $src3:ident, $src4:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let $tgt: $tag = hash4($src1, $src2, $src3, $src4))
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let ($tgt1:ident, $tgt2:ident) = unhash2($src:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let ($tgt1, $tgt2) = unhash2($src) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let ($tgt1:ident, $tgt2:ident, $tgt3:ident) = unhash3($src:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let ($tgt1, $tgt2, $tgt3) = unhash3($src) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let ($tgt1:ident, $tgt2:ident, $tgt3:ident, $tgt4:ident) = unhash4($src:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let ($tgt1, $tgt2, $tgt3, $tgt4) = unhash4($src) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let $tgt:ident = hide($sec:ident, $src:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let $tgt = hide($sec, $src) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, let ($sec:ident, $src:ident) = open($hash:ident) ; $($tail:tt)*) => {
        $crate::ctrl! (
            @seq
            {
                $($limbs)*
                $crate::op!(let ($sec, $src) = open($hash) )
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, match_tag $sii:ident { $( $tag:ident => $case_ops:tt ),* $(,)? } $($tail:tt)*) => {
        $crate::ctrl! (
            @end
            {
                $($limbs)*
            },
            $crate::ctrl!( match_tag $sii { $( $tag => $case_ops ),* } ),
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, match_symbol $sii:ident { $( $symbol:expr => $case_ops:tt ),* , _ => $def:tt $(,)? } $($tail:tt)*) => {
        $crate::ctrl! (
            @end
            {
                $($limbs)*
            },
            $crate::ctrl!( match_symbol $sii { $( $symbol => $case_ops ),* , _ => $def, } ),
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, return ($src1:ident, $src2:ident, $src3:ident) $($tail:tt)*) => {
        $crate::ctrl! (
            @end
            {
                $($limbs)*
            },
            $crate::ctrl!( return ($src1, $src2, $src3) ),
            $($tail)*
        )
    };
    (@seq {$($limbs:expr)*}, $(;)? ) => {
        {
            compile_error!("You must provide Func with a return at each path!");
        }
    };
    (@end { }, $cont:expr,  $(;)?) => {
        {
            $cont
        }
    };
    (@end {$($limbs:expr)+}, $cont:expr,  $(;)?) => {
        {
            let block = $cont;
            let ops = vec!($($limbs),+);
            $crate::lem::Ctrl::Seq(ops, Box::new(block))
        }
    }
}

#[macro_export]
macro_rules! func {
    ($in1:ident $in2:ident $in3:ident $lem:tt) => {
        $crate::lem::Func::new(
            [
                $crate::lem::Var(stringify!($in1).into()),
                $crate::lem::Var(stringify!($in2).into()),
                $crate::lem::Var(stringify!($in3).into()),
            ],
            &$crate::ctrl!($lem),
        )
    };
}

#[cfg(test)]
mod tests {
    use crate::lem::{symbol::Symbol, tag::Tag, Var, Ctrl, Op};

    #[inline]
    fn mptr(name: &str) -> Var {
        Var(name.into())
    }

    #[inline]
    fn match_tag(i: Var, cases: Vec<(Tag, Ctrl)>) -> Ctrl {
        Ctrl::MatchTag(i, indexmap::IndexMap::from_iter(cases))
    }

    #[inline]
    fn match_symbol(i: Var, cases: Vec<(Symbol, Ctrl)>, def: Ctrl) -> Ctrl {
        Ctrl::MatchSymbol(i, indexmap::IndexMap::from_iter(cases), Box::new(def))
    }

    #[test]
    fn test_macros() {
        let lemops = [
            Op::Null(mptr("foo"), Tag::Num),
            Op::Hash2(mptr("foo"), Tag::Char, [mptr("bar"), mptr("baz")]),
            Op::Hash3(
                mptr("foo"),
                Tag::Char,
                [mptr("bar"), mptr("baz"), mptr("bazz")],
            ),
            Op::Hash4(
                mptr("foo"),
                Tag::Char,
                [mptr("bar"), mptr("baz"), mptr("bazz"), mptr("baxx")],
            ),
            Op::Unhash2([mptr("foo"), mptr("goo")], mptr("aaa")),
            Op::Unhash3([mptr("foo"), mptr("goo"), mptr("moo")], mptr("aaa")),
            Op::Unhash4(
                [mptr("foo"), mptr("goo"), mptr("moo"), mptr("noo")],
                mptr("aaa"),
            ),
            Op::Hide(mptr("bar"), mptr("baz"), mptr("bazz")),
            Op::Open(mptr("bar"), mptr("baz"), mptr("bazz")),
        ];
        let lemops_macro = vec![
            op!(let foo: Num),
            op!(let foo: Char = hash2(bar, baz)),
            op!(let foo: Char = hash3(bar, baz, bazz)),
            op!(let foo: Char = hash4(bar, baz, bazz, baxx)),
            op!(let (foo, goo) = unhash2(aaa)),
            op!(let (foo, goo, moo) = unhash3(aaa)),
            op!(let (foo, goo, moo, noo) = unhash4(aaa)),
            op!(let bar = hide(baz, bazz)),
            op!(let (bar, baz) = open(bazz)),
        ];

        for i in 0..9 {
            assert!(lemops[i] == lemops_macro[i]);
        }

        let ret = Ctrl::Return([mptr("bar"), mptr("baz"), mptr("bazz")]);
        let block = Ctrl::Seq(lemops_macro, Box::new(ret));
        let lem_macro_seq = ctrl!({
            let foo: Num;
            let foo: Char = hash2(bar, baz);
            let foo: Char = hash3(bar, baz, bazz);
            let foo: Char = hash4(bar, baz, bazz, baxx);
            let (foo, goo) = unhash2(aaa);
            let (foo, goo, moo) = unhash3(aaa);
            let (foo, goo, moo, noo) = unhash4(aaa);
            let bar = hide(baz, bazz);
            let (bar, baz) = open(bazz);
            return (bar, baz, bazz);
        });

        assert!(block == lem_macro_seq);

        let foo = ctrl!(
            match_tag www {
                Num => {
                    return (foo, foo, foo); // a single Ctrl will not turn into a Seq
                },
                Str => {
                    let foo: Num;
                    return (foo, foo, foo);
                },
                Char => {
                    let foo: Num;
                    let goo: Char;
                    return (foo, goo, goo);
                }
            }
        );
        assert!(
            foo == match_tag(
                mptr("www"),
                vec![
                    (
                        Tag::Num,
                        Ctrl::Return([mptr("foo"), mptr("foo"), mptr("foo")])
                    ),
                    (
                        Tag::Str,
                        Ctrl::Seq(
                            vec![Op::Null(mptr("foo"), Tag::Num)],
                            Box::new(Ctrl::Return([mptr("foo"), mptr("foo"), mptr("foo")]))
                        )
                    ),
                    (
                        Tag::Char,
                        Ctrl::Seq(
                            vec![
                                Op::Null(mptr("foo"), Tag::Num),
                                Op::Null(mptr("goo"), Tag::Char)
                            ],
                            Box::new(Ctrl::Return([mptr("foo"), mptr("goo"), mptr("goo")]))
                        )
                    )
                ]
            )
        );

        let moo = ctrl!(
            match_symbol www {
                Symbol::lurk_sym("nil") => {
                    return (foo, foo, foo); // a single Ctrl will not turn into a Seq
                },
                Symbol::lurk_sym("cons") => {
                    let foo: Num;
                    let goo: Char;
                    return (foo, goo, goo);
                },
                _ => {
                    let xoo: Str;
                    return (xoo, xoo, xoo);
                },
            }
        );

        assert!(
            moo == match_symbol(
                mptr("www"),
                vec![
                    (
                        Symbol::lurk_sym("nil"),
                        Ctrl::Return([mptr("foo"), mptr("foo"), mptr("foo")])
                    ),
                    (
                        Symbol::lurk_sym("cons"),
                        Ctrl::Seq(
                            vec![
                                Op::Null(mptr("foo"), Tag::Num),
                                Op::Null(mptr("goo"), Tag::Char)
                            ],
                            Box::new(Ctrl::Return([mptr("foo"), mptr("goo"), mptr("goo")]))
                        )
                    )
                ],
                Ctrl::Seq(
                    vec![Op::Null(mptr("xoo"), Tag::Str)],
                    Box::new(Ctrl::Return([mptr("xoo"), mptr("xoo"), mptr("xoo")]))
                )
            )
        );
    }
}
