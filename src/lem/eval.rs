use std::collections::HashMap;

use crate::field::LurkField;

use super::{
    lurk_symbol::LurkSymbol, pointers::Ptr, store::Store, tag::Tag, MetaPtr, Witness, LEM, LEMOP,
};

// TODO: remove name conflicts between branches automatically instead of putting
// this burden on the LEM programmer's shoulders
pub fn step<F: LurkField>() -> LEM<'static, F> {
    let input = ["expr_in", "env_in", "cont_in"];
    let lem_op = LEMOP::mk_match_tag(
        MetaPtr("expr_in"),
        vec![(
            Tag::Num,
            LEMOP::mk_match_tag(
                MetaPtr("cont_in"),
                vec![(
                    Tag::Outermost,
                    LEMOP::Seq(vec![
                        LEMOP::Copy(MetaPtr("expr_out_ret"), MetaPtr("expr_in")),
                        LEMOP::Copy(MetaPtr("env_out_ret"), MetaPtr("env_in")),
                        LEMOP::MkNull(MetaPtr("cont_out_ret"), Tag::Terminal),
                    ]),
                )],
                LEMOP::Seq(vec![
                    LEMOP::Copy(MetaPtr("expr_out_error_inner"), MetaPtr("expr_in")),
                    LEMOP::Copy(MetaPtr("env_out_error_inner"), MetaPtr("env_in")),
                    LEMOP::MkNull(MetaPtr("cont_out_error_inner"), Tag::Error),
                ]),
            ),
        )],
        LEMOP::Seq(vec![
            LEMOP::Copy(MetaPtr("expr_out_error_outer"), MetaPtr("expr_in")),
            LEMOP::Copy(MetaPtr("env_out_error_outer"), MetaPtr("env_in")),
            LEMOP::MkNull(MetaPtr("cont_out_error_outer"), Tag::Error),
        ]),
    );
    let do_copy = vec![
        (
            vec![
                "expr_out_ret",
                "expr_out_error_inner",
                "expr_out_error_outer",
            ],
            "expr_out",
        ),
        (
            vec!["env_out_ret", "env_out_error_inner", "env_out_error_outer"],
            "env_out",
        ),
        (
            vec![
                "cont_out_ret",
                "cont_out_error_inner",
                "cont_out_error_outer",
            ],
            "cont_out",
        ),
    ];
    let output = ["expr_out", "env_out", "cont_out"];
    LEM {
        input,
        lem_op,
        do_copy,
        output,
    }
}

pub fn eval<'a, F: LurkField>(
    lem: LEM<'a, F>,
    expr: Ptr<F>,
) -> Result<(Vec<Witness<'a, F>>, Store<F>), String> {
    let mut expr = expr;
    let mut env = Ptr::lurk_sym(LurkSymbol::Nil);
    let mut cont = Ptr::null(Tag::Outermost);
    let mut steps_data = vec![];
    let mut store: Store<F> = Default::default();
    let terminal = Ptr::null(Tag::Terminal);
    loop {
        let input = [expr, env, cont];
        let (output, ptrs, vars) = lem.run(input, &mut store)?;
        steps_data.push(Witness {
            input,
            output,
            ptrs,
            vars,
        });
        if output[2] == terminal {
            break;
        } else {
            [expr, env, cont] = output;
        }
    }
    Ok((steps_data, store))
}

pub fn eval_res<'a, F: LurkField>(
    lem: LEM<'a, F>,
    expr: Ptr<F>,
) -> Result<(Ptr<F>, Store<F>), String> {
    let (steps_data, store) = eval(lem, expr)?;
    Ok((
        steps_data
            .last()
            .expect("eval should add at least one step data")
            .output[0],
        store,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lem::{
        pointers::{Ptr, PtrVal},
        tag::Tag,
    };
    use blstrs::Scalar;

    #[test]
    fn check_step() {
        step::<Scalar>().check()
    }

    #[test]
    fn eval_42() {
        let expr = Ptr {
            tag: Tag::Num,
            val: PtrVal::Field(Scalar::from(42)),
        };
        let (res, _) = eval_res(step(), expr).unwrap();
        assert!(res == expr);
    }
}
