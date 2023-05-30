macro_rules! metaptr {
    ($variable:ident) => {
        crate::lem::MetaPtr(stringify!($variable).to_string())
    };
}

macro_rules! metaptrs {
    ($($variable:ident),*) => {
        [
            $(metaptr!($variable)),*
        ]
    };
}

#[macro_export]
macro_rules! lemop {
    ( $tag:ident $tgt:ident = null ) => {
        crate::lem::LEMOP::MkNull(
            metaptr!($tgt),
            crate::lem::Tag::$tag,
        )
    };
    ( $tag:ident $tgt:ident = hash2 $src1:ident $src2:ident ) => {
        crate::lem::LEMOP::Hash2Ptrs(
            metaptr!($tgt),
            crate::lem::Tag::$tag,
            metaptrs!($src1, $src2),
        )
    };
    ( $tag:ident $tgt:ident = hash3 $src1:ident $src2:ident $src3:ident ) => {
        crate::lem::LEMOP::Hash3Ptrs(
            metaptr!($tgt),
            crate::lem::Tag::$tag,
            metaptrs!($src1, $src2, $src3),
        )
    };
    ( $tag:ident $tgt:ident = hash4 $src1:ident $src2:ident $src3:ident $src4:ident ) => {
        crate::lem::LEMOP::Hash4Ptrs(
            metaptr!($tgt),
            crate::lem::Tag::$tag,
            metaptrs!( $src1, $src2, $src3, $src4),
        )
    };
    ( $tgt1:ident $tgt2:ident = unhash2 $src:ident ) => {
        crate::lem::LEMOP::Unhash2Ptrs(
            metaptrs!( $tgt1, $tgt2),
            crate::lem::MetaPtr(stringify!($src).to_string()),
        )
    };
    ( $tgt1:ident $tgt2:ident $tgt3:ident = unhash3 $src:ident ) => {
        crate::lem::LEMOP::Unhash3Ptrs(
            metaptrs!( $tgt1, $tgt2, $tgt3),
            metaptr!( $src),
        )
    };
    ( $tgt1:ident $tgt2:ident $tgt3:ident $tgt4:ident = unhash4 $src:ident ) => {
        crate::lem::LEMOP::Unhash4Ptrs(
            metaptrs!( $tgt1, $tgt2, $tgt3, $tgt4),
            metaptr!( $src),
        )
    };
    ( $tgt:ident = hide $sec:ident $src:ident ) => {
        crate::lem::LEMOP::Hide(
           metaptr!($tgt), metaptr!($sec), metaptr!($src),
        )
    };
    ( $sec:ident $src:ident = open $hash:ident ) => {
        crate::lem::LEMOP::Open(
            metaptr!($sec), metaptr!($src), metaptr!($hash),
        )
    };
    ( return $src1:ident $src2:ident $src3:ident ) => {
        crate::lem::LEMOP::Return(
            metaptrs!($src1, $src2, $src3)
        )
    };
    // ( match_tag $ptr:ident { $($tag:ident => { $($body:tt)* }),* } ) => {
        // TODO
    // };
    // seq entry point, with a separate bracketing to differentiate
    ({ $($body:tt)* }) => {
        {
            lemop! ( @seq {}, $($body)* )
        }
    };
    // termination rule: we run out of input modulo trailing semicolumn, so we construct the Seq
    // Note the bracketed limbs pattern, which disambiguates wrt the last argument
    (@seq {$($limbs:tt)*}, $(;)? ) => {
        LEMOP::Seq(vec!($(
            $limbs
        )*))
    };
    // handle the recursion: as we see a statement, we push it to the limbs position in the pattern
    (@seq {$($limbs:tt)*}, $tag:ident $tgt:ident = null ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!($tag $tgt = null),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tag:ident $tgt:ident = hash2 $src1:ident $src2:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tag $tgt = hash2 $src1 $src2 ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tag:ident $tgt:ident = hash3 $src1:ident $src2:ident $src3:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tag $tgt = hash3 $src1 $src2 $src3 ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tag:ident $tgt:ident = hash4 $src1:ident $src2:ident $src3:ident $src4:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tag $tgt = hash4 $src1 $src2 $src3 $src4 ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tgt1:ident $tgt2:ident = unhash2 $src:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tgt1 $tgt2 = unhash2 $src ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tgt1:ident $tgt2:ident $tgt3:ident = unhash3 $src:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tgt1 $tgt2 $tgt3 = unhash3 $src ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tgt1:ident $tgt2:ident $tgt3:ident $tgt4:ident = unhash4 $src:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tgt1 $tgt2 $tgt3 $tgt4 = unhash4 $src ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $tgt:ident = hide $sec:ident $src:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $tgt = hide $sec $src ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, $sec:ident $src:ident = open $hash:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( $sec $src = open $hash ),
            },
            $($tail)*
        )
    };
    (@seq {$($limbs:tt)*}, return $src1:ident $src2:ident $src3:ident ; $($tail:tt)*) => {
        lemop! (
            @seq
            {
                $($limbs)*
                lemop!( return $src1 $src2 $src3 ),
            },
            $($tail)*
        )
    };
}

#[cfg(test)]
mod tests {
    use crate::lem::{shortcuts::mptr, tag::Tag, LEMOP};

    #[test]
    fn test_macros() {
        let lemops = [
            LEMOP::MkNull(mptr("foo"), Tag::Num),
            LEMOP::Hash2Ptrs(mptr("foo"), Tag::Char, [mptr("bar"), mptr("baz")]),
            LEMOP::Hash3Ptrs(
                mptr("foo"),
                Tag::Char,
                [mptr("bar"), mptr("baz"), mptr("bazz")],
            ),
            LEMOP::Hash4Ptrs(
                mptr("foo"),
                Tag::Char,
                [mptr("bar"), mptr("baz"), mptr("bazz"), mptr("baxx")],
            ),
            LEMOP::Unhash2Ptrs([mptr("foo"), mptr("goo")], mptr("aaa")),
            LEMOP::Unhash3Ptrs([mptr("foo"), mptr("goo"), mptr("moo")], mptr("aaa")),
            LEMOP::Unhash4Ptrs(
                [mptr("foo"), mptr("goo"), mptr("moo"), mptr("noo")],
                mptr("aaa"),
            ),
            LEMOP::Hide(mptr("bar"), mptr("baz"), mptr("bazz")),
            LEMOP::Open(mptr("bar"), mptr("baz"), mptr("bazz")),
            LEMOP::Return([mptr("bar"), mptr("baz"), mptr("bazz")]),
        ];
        let lemops_macro = [
            lemop!(Num foo = null),
            lemop!(Char foo = hash2 bar baz),
            lemop!(Char foo = hash3 bar baz bazz),
            lemop!(Char foo = hash4 bar baz bazz baxx),
            lemop!(foo goo = unhash2 aaa),
            lemop!(foo goo moo = unhash3 aaa),
            lemop!(foo goo moo noo = unhash4 aaa),
            lemop!(bar = hide baz bazz),
            lemop!(bar baz = open bazz),
            lemop!(return bar baz bazz),
        ];

        for i in 0..10 {
            assert!(lemops[i] == lemops_macro[i]);
        }

        let lemop_macro_seq = lemop!({
            Num foo = null;
            Char foo = hash2 bar baz;
            Char foo = hash3 bar baz bazz;
            Char foo = hash4 bar baz bazz baxx;
            foo goo = unhash2 aaa;
            foo goo moo = unhash3 aaa;
            foo goo moo noo = unhash4 aaa;
            bar = hide baz bazz;
            bar baz = open bazz;
            return bar baz bazz;
        });

        assert!(LEMOP::Seq(lemops.to_vec()) == lemop_macro_seq);

        trace_macros!(true);
        // let foo = lemop!(
        //    match_tag www {
        //        Str => {
        //            Num foo = null;
        //        },
        //    }
        // );
        trace_macros!(false);
    }
}
