use abomonation::Abomonation;
use bellpepper::util_cs::witness_cs::WitnessCS;
use bellpepper_core::ConstraintSystem;
use nova::traits::Group;
use std::sync::Arc;

use crate::coprocessor::Coprocessor;
use crate::eval::{lang::Lang, Meta};
use crate::proof::{supernova::FoldingConfig, MultiFrameTrait, Prover};
use std::cell::RefCell;
use std::rc::Rc;
use tracing_test::traced_test;

use crate::lurk_sym_ptr;
use crate::num::Num;
use crate::proof::nova::*;
use crate::state::{user_sym, State};
use crate::store::Store;

use crate::eval::lang::Coproc;
use crate::field::LurkField;
use crate::proof::{CEKState, EvaluationStore};
use crate::tag::{Op, Op1, Op2};

use bellpepper::util_cs::{metric_cs::MetricCS, Comparable};
use bellpepper_core::test_cs::TestConstraintSystem;
use bellpepper_core::Delta;
use pasta_curves::pallas::Scalar as Fr;

pub const DEFAULT_REDUCTION_COUNT: usize = 5;
const REDUCTION_COUNTS_TO_TEST: [usize; 3] = [1, 2, 5];

// Returns index of first mismatch, along with the mismatched elements if they exist.
fn mismatch<T: PartialEq + Copy>(a: &[T], b: &[T]) -> Option<(usize, (Option<T>, Option<T>))> {
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        if a[i] != b[i] {
            return Some((i, (Some(a[i]), Some(b[i]))));
        }
    }
    match (a.get(min_len), b.get(min_len)) {
        (Some(&a_elem), None) => Some((min_len, (Some(a_elem), None))),
        (None, Some(&b_elem)) => Some((min_len, (None, Some(b_elem)))),
        _ => None,
    }
}

pub fn test_aux<'a, F: CurveCycleEquipped, C: Coprocessor<F>, M: MultiFrameTrait<'a, F, C>>(
    s: &'a M::Store,
    expr: &str,
    expected_result: Option<M::Ptr>,
    expected_env: Option<M::Ptr>,
    expected_cont: Option<M::ContPtr>,
    expected_emitted: Option<&[M::Ptr]>,
    expected_iterations: usize,
    lang: Option<Arc<Lang<F, C>>>,
)
// technical bounds that would disappear once associated_type_bounds stabilizes
where
    <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
{
    for chunk_size in REDUCTION_COUNTS_TO_TEST {
        nova_test_full_aux::<F, C, M>(
            s,
            expr,
            expected_result,
            expected_env,
            expected_cont,
            expected_emitted,
            expected_iterations,
            chunk_size,
            false,
            None,
            lang.clone(),
        )
    }
}

pub fn nova_test_full_aux<
    'a,
    F: CurveCycleEquipped,
    C: Coprocessor<F>,
    M: MultiFrameTrait<'a, F, C>,
>(
    s: &'a M::Store,
    expr: &str,
    expected_result: Option<M::Ptr>,
    expected_env: Option<M::Ptr>,
    expected_cont: Option<M::ContPtr>,
    expected_emitted: Option<&[M::Ptr]>,
    expected_iterations: usize,
    reduction_count: usize,
    check_nova: bool,
    limit: Option<usize>,
    lang: Option<Arc<Lang<F, C>>>,
)
// technical bounds that would disappear once associated_type_bounds stabilizes
where
    <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
{
    let expr = s.read(expr).unwrap();

    let f = |l| {
        nova_test_full_aux2::<F, C, M>(
            s,
            expr,
            expected_result,
            expected_env,
            expected_cont,
            expected_emitted,
            expected_iterations,
            reduction_count,
            check_nova,
            limit,
            l,
        )
    };

    if let Some(l) = lang {
        f(l)
    } else {
        let lang = Lang::new();
        f(Arc::new(lang))
    };
}

pub fn nova_test_full_aux2<
    'a,
    F: CurveCycleEquipped,
    C: Coprocessor<F>,
    M: MultiFrameTrait<'a, F, C>,
>(
    s: &'a M::Store,
    expr: M::Ptr,
    expected_result: Option<M::Ptr>,
    expected_env: Option<M::Ptr>,
    expected_cont: Option<M::ContPtr>,
    expected_emitted: Option<&[M::Ptr]>,
    expected_iterations: usize,
    reduction_count: usize,
    check_nova: bool,
    limit: Option<usize>,
    lang: Arc<Lang<F, C>>,
)
// technical bounds that would disappear once associated_type_bounds stabilizes
where
    <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
{
    let limit = limit.unwrap_or(10000);

    let e = s.initial_empty_env();

    let nova_prover = NovaProver::<'a, F, C, M>::new(reduction_count, (*lang).clone());
    let frames = M::get_evaluation_frames(
        |frame_count| nova_prover.needs_frame_padding(frame_count),
        expr,
        e,
        s,
        limit,
        &lang,
    )
    .unwrap();

    if check_nova {
        let pp = public_params::<_, _, M>(reduction_count, lang.clone());
        let (proof, z0, zi, num_steps) = nova_prover.prove(&pp, &frames, s, &lang).unwrap();

        let res = proof.verify(&pp, num_steps, &z0, &zi);
        if res.is_err() {
            tracing::debug!("{:?}", &res);
        }
        assert!(res.unwrap());

        let compressed = proof.compress(&pp).unwrap();
        let res2 = compressed.verify(&pp, num_steps, &z0, &zi);

        assert!(res2.unwrap());
    }

    let folding_config = Arc::new(FoldingConfig::new_ivc(lang, nova_prover.reduction_count()));

    let multiframes = M::from_frames(
        nova_prover.reduction_count(),
        &frames,
        s,
        folding_config.clone(),
    );
    let len = multiframes.len();

    let adjusted_iterations = nova_prover.expected_total_iterations(expected_iterations);
    let mut previous_frame: Option<&M> = None;

    let mut cs_blank = MetricCS::<F>::new();

    let blank = M::blank(folding_config, Meta::Lurk);
    blank
        .synthesize(&mut cs_blank)
        .expect("failed to synthesize blank");

    for (_i, multiframe) in multiframes.iter().enumerate() {
        let mut cs = TestConstraintSystem::new();
        let mut wcs = WitnessCS::new();

        tracing::debug!("synthesizing test cs");
        multiframe.clone().synthesize(&mut cs).unwrap();
        tracing::debug!("synthesizing witness cs");
        multiframe.clone().synthesize(&mut wcs).unwrap();

        if let Some(prev) = previous_frame {
            assert!(prev.precedes(multiframe));
        };
        // tracing::debug!("frame {}" i);
        let unsat = cs.which_is_unsatisfied();
        if unsat.is_some() {
            // For some reason, this isn't getting printed from within the implementation as expected.
            // Since we always want to know this information, if the condition occurs, just print it here.
            tracing::debug!("{:?}", unsat);
        }
        assert!(cs.is_satisfied());
        assert!(cs.verify(&multiframe.public_inputs()));
        tracing::debug!("cs is satisfied!");
        let cs_inputs = cs.scalar_inputs();
        let cs_aux = cs.scalar_aux();

        let wcs_inputs = wcs.scalar_inputs();
        let wcs_aux = wcs.scalar_aux();

        assert_eq!(None, mismatch(&cs_inputs, &wcs_inputs));
        assert_eq!(None, mismatch(&cs_aux, &wcs_aux));

        previous_frame = Some(multiframe);

        let delta = cs.delta(&cs_blank, true);

        assert!(delta == Delta::Equal);
    }
    let output = previous_frame.unwrap().output().as_ref().unwrap();

    if let Some(expected_emitted) = expected_emitted {
        let mut emitted_vec = Vec::default();
        for frame in frames.iter() {
            emitted_vec.extend(M::emitted(s, frame));
        }
        assert_eq!(expected_emitted, &emitted_vec);
    }

    if let Some(expected_result) = expected_result {
        assert!(s.ptr_eq(&expected_result, output.expr()).unwrap());
    }
    if let Some(expected_env) = expected_env {
        assert!(s.ptr_eq(&expected_env, output.env()).unwrap());
    }
    if let Some(expected_cont) = expected_cont {
        assert_eq!(&expected_cont, output.cont());
    } else {
        assert_eq!(&s.get_cont_terminal(), output.cont());
    }

    assert_eq!(expected_iterations, M::significant_frame_count(&frames));
    assert_eq!(adjusted_iterations, len);
}

// IMPORTANT: Run next tests at least once. Some are ignored because they
// are expensive. The criteria is that if the number of iteractions is
// more than 30 we ignore it.
////////////////////////////////////////////////////////////////////////////

type M1<'a, Fr> = C1Lurk<'a, Fr, Coproc<Fr>>;

#[test]
fn test_prove_binop() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(+ 1 2)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
#[should_panic]
// This tests the testing mechanism. Since the supplied expected value is wrong,
// the test should panic on an assertion failure.
fn test_prove_binop_fail() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(+ 1 2)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_arithmetic_let() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((a 5)
                    (b 1)
                    (c 2))
                (/ (+ a b) c))",
        Some(expected),
        None,
        Some(terminal),
        None,
        18,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_eq() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(eq 5 5)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        DEFAULT_REDUCTION_COUNT,
        true,
        None,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_num_equal() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(= 5 5)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );

    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(= 5 6)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_invalid_num_equal() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(= 5 nil)",
        Some(expected),
        None,
        Some(error),
        None,
        3,
        None,
    );

    let expected = s.num(5);
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(= nil 5)",
        Some(expected),
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_equal() {
    let s = &mut Store::<Fr>::default();
    let nil = lurk_sym_ptr!(s, nil);
    let t = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(
        s,
        "(eq 5 nil)",
        Some(nil),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(eq nil 5)",
        Some(nil),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(eq nil nil)",
        Some(t),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(s, "(eq 5 5)", Some(t), None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_quote_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(quote (1) (2))", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_if() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(5);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(if t 5 6)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );

    let expected = s.num(6);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(if nil 5 6)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    )
}

#[test]
fn test_prove_if_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(5);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(if nil 5 6 7)",
        Some(expected),
        None,
        Some(error),
        None,
        2,
        None,
    )
}

#[test]
#[ignore]
fn test_prove_if_fully_evaluates() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(10);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(if t (+ 5 5) 6)",
        Some(expected),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
#[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
fn test_prove_recursion1() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base)
                            (lambda (exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base ((exp base) (- exponent 1))))))))
                ((exp 5) 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        66,
        None,
    );
}

#[test]
#[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
fn test_prove_recursion2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base)
                                (lambda (exponent)
                                    (lambda (acc)
                                    (if (= 0 exponent)
                                        acc
                                        (((exp base) (- exponent 1)) (* acc base))))))))
            (((exp 5) 2) 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        93,
        None,
    );
}

fn test_prove_unop_regression_aux(chunk_count: usize) {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(atom 123)",
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        chunk_count, // This needs to be 1 to exercise the bug.
        false,
        None,
        None,
    );

    let expected = s.num(1);
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(car '(1 . 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        chunk_count, // This needs to be 1 to exercise the bug.
        false,
        None,
        None,
    );

    let expected = s.num(2);
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(cdr '(1 . 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        chunk_count, // This needs to be 1 to exercise the bug.
        false,
        None,
        None,
    );

    let expected = s.num(123);
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(emit 123)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        chunk_count,
        false,
        None,
        None,
    )
}

#[test]
#[ignore]
fn test_prove_unop_regression() {
    // We need to at least use chunk size 1 to exercise the regression.
    // Also use a non-1 value to check the MultiFrame case.
    for i in 1..2 {
        test_prove_unop_regression_aux(i);
    }
}

#[test]
#[ignore]
fn test_prove_emit_output() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(emit 123)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(99);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x) x) 99)",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(99);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (y)
                ((lambda (x) y) 888))
                99)",
        Some(expected),
        None,
        Some(terminal),
        None,
        9,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate3() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(999);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (y)
                    ((lambda (x)
                    ((lambda (z) z)
                        x))
                    y))
                999)",
        Some(expected),
        None,
        Some(terminal),
        None,
        10,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate4() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(888);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (y)
                    ((lambda (x)
                    ((lambda (z) z)
                        x))
                    ;; NOTE: We pass a different value here.
                    888))
                999)",
        Some(expected),
        None,
        Some(terminal),
        None,
        10,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate5() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(999);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(((lambda (fn)
                    (lambda (x) (fn x)))
                (lambda (y) y))
                999)",
        Some(expected),
        None,
        Some(terminal),
        None,
        13,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_evaluate_sum() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(9);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(+ 2 (+ 3 4))",
        Some(expected),
        None,
        Some(terminal),
        None,
        6,
        None,
    );
}

#[test]
fn test_prove_binop_rest_is_nil() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(9);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(- 9 8 7)",
        Some(expected),
        None,
        Some(error),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(= 9 8 7)",
        Some(expected),
        None,
        Some(error),
        None,
        2,
        None,
    );
}

fn op_syntax_error<T: Op + Copy>() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    let test = |op: T| {
        let name = op.symbol_name();

        if !op.supports_arity(0) {
            let expr = format!("({name})");
            tracing::debug!("{:?}", &expr);
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        if !op.supports_arity(1) {
            let expr = format!("({name} 123)");
            tracing::debug!("{:?}", &expr);
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        if !op.supports_arity(2) {
            let expr = format!("({name} 123 456)");
            tracing::debug!("{:?}", &expr);
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }

        if !op.supports_arity(3) {
            let expr = format!("({name} 123 456 789)");
            tracing::debug!("{:?}", &expr);
            let iterations = if op.supports_arity(2) { 2 } else { 1 };
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, iterations, None);
        }
    };

    for op in T::all() {
        test(*op);
    }
}

#[test]
#[ignore]
fn test_prove_unop_syntax_error() {
    op_syntax_error::<Op1>();
}

#[test]
#[ignore]
fn test_prove_binop_syntax_error() {
    op_syntax_error::<Op2>();
}

#[test]
fn test_prove_diff() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(4);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(- 9 5)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_product() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(45);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(* 9 5)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_quotient() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(7);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(/ 21 3)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_error_div_by_zero() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(0);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(/ 21 0)",
        Some(expected),
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_error_invalid_type_and_not_cons() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(/ 21 nil)",
        Some(expected),
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_adder() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(5);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(((lambda (x)
                (lambda (y)
                    (+ x y)))
                2)
                3)",
        Some(expected),
        None,
        Some(terminal),
        None,
        13,
        None,
    );
}

#[test]
fn test_prove_current_env_simple() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(current-env)",
        Some(expected),
        None,
        Some(terminal),
        None,
        1,
        None,
    );
}

#[test]
fn test_prove_current_env_rest_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let expected = s.read("(current-env a)").unwrap();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(current-env a)",
        Some(expected),
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_let_simple() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(1);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((a 1))
                a)",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_let_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((a 1 2)) a)",
        None,
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
fn test_prove_letrec_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((a 1 2)) a)",
        None,
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
fn test_prove_lambda_empty_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x)) 0)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_let_empty_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(let)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_let_empty_body_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(let ((a 1)))", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_letrec_empty_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(letrec)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_letrec_empty_body_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((a 1)))",
        None,
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
fn test_prove_let_body_nil() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(eq nil (let () nil))",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}

#[test]
fn test_prove_let_rest_body_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((a 1)) a 1)",
        None,
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
fn test_prove_letrec_rest_body_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((a 1)) a 1)",
        None,
        None,
        Some(error),
        None,
        1,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_let_null_bindings() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let () (+ 1 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}
#[test]
#[ignore]
fn test_prove_letrec_null_bindings() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec () (+ 1 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_let() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(6);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((a 1)
                    (b 2)
                    (c 3))
                (+ a (+ b c)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        18,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_arithmetic() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(20);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((((lambda (x)
                    (lambda (y)
                    (lambda (z)
                        (* z
                            (+ x y)))))
                2)
                3)
                4)",
        Some(expected),
        None,
        Some(terminal),
        None,
        23,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_comparison() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((x 2)
                    (y 3)
                    (z 4))
                (= 20 (* z
                        (+ x y))))",
        Some(expected),
        None,
        Some(terminal),
        None,
        21,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_conditional() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(5);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    (false (lambda (a)
                            (lambda (b)
                                b)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) true))",
        Some(expected),
        None,
        Some(terminal),
        None,
        35,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_conditional2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(6);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    (false (lambda (a)
                            (lambda (b)
                                b)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) false))",
        Some(expected),
        None,
        Some(terminal),
        None,
        32,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_fundamental_conditional_bug() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(5);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) true))",
        Some(expected),
        None,
        Some(terminal),
        None,
        32,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_fully_evaluates() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(10);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(if t (+ 5 5) 6)",
        Some(expected),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_recursion() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base)
                                (lambda (exponent)
                                    (if (= 0 exponent)
                                        1
                                        (* base ((exp base) (- exponent 1))))))))
                        ((exp 5) 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        66,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_recursion_multiarg() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp base (- exponent 1)))))))
                        (exp 5 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        69,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_recursion_optimized() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((exp (lambda (base)
                            (letrec ((base-inner
                                        (lambda (exponent)
                                            (if (= 0 exponent)
                                                1
                                                (* base (base-inner (- exponent 1)))))))
                                    base-inner))))
                ((exp 5) 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        56,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_tail_recursion() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base)
                                (lambda (exponent-remaining)
                                    (lambda (acc)
                                    (if (= 0 exponent-remaining)
                                        acc
                                        (((exp base) (- exponent-remaining 1)) (* acc base))))))))
                        (((exp 5) 2) 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        93,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_tail_recursion_somewhat_optimized() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(25);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base)
                                (letrec ((base-inner
                                            (lambda (exponent-remaining)
                                            (lambda (acc)
                                                (if (= 0 exponent-remaining)
                                                    acc
                                                    ((base-inner (- exponent-remaining 1)) (* acc base)))))))
                                        base-inner))))
                        (((exp 5) 2) 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        81,None
    );
}

#[test]
#[ignore]
fn test_prove_no_mutual_recursion() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((even (lambda (n)
                                (if (= 0 n)
                                    t
                                    (odd (- n 1)))))
                        (odd (lambda (n)
                                (even (- n 1)))))
                    ;; NOTE: This is not true mutual-recursion.
                    ;; However, it exercises the behavior of LETREC.
                    (odd 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        22,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_no_mutual_recursion_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((even (lambda (n)
                                (if (= 0 n)
                                    t
                                    (odd (- n 1)))))
                        (odd (lambda (n)
                                (even (- n 1)))))
                    ;; NOTE: This is not true mutual-recursion.
                    ;; However, it exercises the behavior of LETREC.
                    (odd 2))",
        None,
        None,
        Some(error),
        None,
        25,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_cons1() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(1);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(car (cons 1 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
fn test_prove_car_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(car (1 2) 3)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_cdr_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(cdr (1 2) 3)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_atom_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(atom 123 4)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_emit_end_is_nil_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(emit 123 4)", None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_cons2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(cdr (cons 1 2))",
        Some(expected),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
fn test_prove_zero_arg_lambda1() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda () 123))",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_zero_arg_lambda2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(10);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((x 9) (f (lambda () (+ x 1)))) (f))",
        Some(expected),
        None,
        Some(terminal),
        None,
        10,
        None,
    );
}

#[test]
fn test_prove_zero_arg_lambda3() {
    let s = &mut Store::<Fr>::default();
    let expected = {
        let arg = s.user_sym("x");
        let num = s.num(123);
        let body = s.list(&[num]);
        let env = lurk_sym_ptr!(s, nil);
        s.intern_fun(arg, body, env)
    };
    let terminal = s.get_cont_terminal();
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x) 123))",
        Some(expected),
        None,
        Some(terminal),
        None,
        3,
        DEFAULT_REDUCTION_COUNT,
        false,
        None,
        None,
    );
}

#[test]
fn test_prove_zero_arg_lambda4() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda () 123) 1)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_zero_arg_lambda5() {
    let s = &mut Store::<Fr>::default();
    let expected = s.read("(123)").unwrap();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, "(123)", Some(expected), None, Some(error), None, 1, None);
}

#[test]
fn test_prove_zero_arg_lambda6() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((emit 123))",
        Some(expected),
        None,
        Some(error),
        None,
        5,
        None,
    );
}

#[test]
fn test_prove_nested_let_closure_regression() {
    let s = &mut Store::<Fr>::default();
    let terminal = s.get_cont_terminal();
    let expected = s.num(6);
    let expr = "(let ((data-function (lambda () 123))
                        (x 6)
                        (data (data-function)))
                    x)";
    test_aux::<_, _, M1<'_, _>>(
        s,
        expr,
        Some(expected),
        None,
        Some(terminal),
        None,
        14,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_minimal_tail_call() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec
                ((f (lambda (x)
                        (if (= x 3)
                            123
                            (f (+ x 1))))))
                (f 0))",
        Some(expected),
        None,
        Some(terminal),
        None,
        50,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_cons_in_function1() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(((lambda (a)
                (lambda (b)
                    (car (cons a b))))
                2)
                3)",
        Some(expected),
        None,
        Some(terminal),
        None,
        15,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_cons_in_function2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(((lambda (a)
                (lambda (b)
                    (cdr (cons a b))))
                2)
                3)",
        Some(expected),
        None,
        Some(terminal),
        None,
        15,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_multiarg_eval_bug() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(car (cdr '(1 2 3 4)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_multiple_letrec_bindings() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec
                ((x 888)
                (f (lambda (x)
                        (if (= x 5)
                            123
                            (f (+ x 1))))))
                (f 0))",
        Some(expected),
        None,
        Some(terminal),
        None,
        78,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_tail_call2() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec
                ((f (lambda (x)
                        (if (= x 5)
                            123
                            (f (+ x 1)))))
                (g (lambda (x) (f x))))
                (g 0))",
        Some(expected),
        None,
        Some(terminal),
        None,
        84,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_multiple_letrecstar_bindings() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(13);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((double (lambda (x) (* 2 x)))
                        (square (lambda (x) (* x x))))
                        (+ (square 3) (double 2)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        22,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_multiple_letrecstar_bindings_referencing() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(11);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((double (lambda (x) (* 2 x)))
                        (double-inc (lambda (x) (+ 1 (double x)))))
                        (+ (double 3) (double-inc 2)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        31,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_multiple_letrecstar_bindings_recursive() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(33);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((exp (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp base (- exponent 1))))))
                        (exp2 (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp2 base (- exponent 1))))))
                        (exp3 (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp3 base (- exponent 1)))))))
                        (+ (+ (exp 3 2) (exp2 2 3))
                        (exp3 4 2)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        242,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_dont_discard_rest_env() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(18);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((z 9))
                (letrec ((a 1)
                            (b 2)
                            (l (lambda (x) (+ z x))))
                        (l 9)))",
        Some(expected),
        None,
        Some(terminal),
        None,
        22,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_fibonacci() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(1);
    let terminal = s.get_cont_terminal();
    nova_test_full_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((next (lambda (a b n target)
                    (if (eq n target)
                        a
                        (next b
                            (+ a b)
                            (+ 1 n)
                        target))))
                (fib (next 0 1 0)))
            (fib 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        89,
        5,
        false,
        None,
        None,
    );
}

// #[test]
// #[ignore]
// fn test_prove_fibonacci_100() {
//     let s = &mut Store::<Fr>::default();
//     let expected = s.read("354224848179261915075").unwrap();
//     let terminal = s.get_cont_terminal();
//     nova_test_full_aux::<Coproc<Fr>>::(
//         s,
//         "(letrec ((next (lambda (a b n target)
//                  (if (eq n target)
//                      a
//                      (next b
//                          (+ a b)
//                          (+ 1 n)
//                         target))))
//                 (fib (next 0 1 0)))
//             (fib 100))",
//         Some(expected),
//         None,
//         Some(terminal),
//         None,
//         4841,
//         5,
//         false,
//     );
// }

#[test]
fn test_prove_terminal_continuation_regression() {
    let s = &mut Store::<Fr>::default();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((a (lambda (x) (cons 2 2))))
            (a 1))",
        None,
        None,
        Some(terminal),
        None,
        9,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_chained_functional_commitment() {
    let s = &mut Store::<Fr>::default();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((secret 12345)
                    (a (lambda (acc x)
                        (let ((acc (+ acc x)))
                            (cons acc (cons secret (a acc)))))))
            (a 0 5))",
        None,
        None,
        Some(terminal),
        None,
        39,
        None,
    );
}

#[test]
fn test_prove_begin_empty() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(begin)",
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_begin_emit() {
    let s = &mut Store::<Fr>::default();
    let expr = "(begin (emit 1) (emit 2) (emit 3))";
    let expected_expr = s.num(3);
    let expected_emitted = vec![s.num(1), s.num(2), s.num(3)];
    test_aux::<_, _, M1<'_, _>>(
        s,
        expr,
        Some(expected_expr),
        None,
        None,
        Some(&expected_emitted),
        13,
        None,
    );
}

#[test]
fn test_prove_str_car() {
    let s = &mut Store::<Fr>::default();
    let expected_a = s.read(r"#\a").unwrap();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car "apple")"#,
        Some(expected_a),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_str_cdr() {
    let s = &mut Store::<Fr>::default();
    let expected_pple = s.read(r#" "pple" "#).unwrap();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr "apple")"#,
        Some(expected_pple),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_str_car_empty() {
    let s = &mut Store::<Fr>::default();
    let expected_nil = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car "")"#,
        Some(expected_nil),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_str_cdr_empty() {
    let s = &mut Store::<Fr>::default();
    let expected_empty_str = s.intern_string("");
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr "")"#,
        Some(expected_empty_str),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_strcons() {
    let s = &mut Store::<Fr>::default();
    let expected_apple = s.read(r#" "apple" "#).unwrap();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(strcons #\a "pple")"#,
        Some(expected_apple),
        None,
        Some(terminal),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_str_cons_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r"(strcons #\a 123)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
fn test_prove_one_arg_cons_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, r#"(cons "")"#, None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_car_nil() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car nil)"#,
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_cdr_nil() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr nil)"#,
        Some(expected),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_car_cdr_invalid_tag_error_sym() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, r#"(car car)"#, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, r#"(cdr car)"#, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_car_cdr_invalid_tag_error_char() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, r"(car #\a)", None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, r"(cdr #\a)", None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_car_cdr_invalid_tag_error_num() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, r#"(car 42)"#, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, r#"(cdr 42)"#, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_car_cdr_of_cons() {
    let s = &mut Store::<Fr>::default();
    let res1 = s.num(1);
    let res2 = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car (cons 1 2))"#,
        Some(res1),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr (cons 1 2))"#,
        Some(res2),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
fn test_prove_car_cdr_invalid_tag_error_lambda() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car (lambda (x) x))"#,
        None,
        None,
        Some(error),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr (lambda (x) x))"#,
        None,
        None,
        Some(error),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_hide_open() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (hide 123 456))";
    let expected = s.num(456);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 5, None);
}

#[test]
fn test_prove_hide_wrong_secret_type() {
    let s = &mut Store::<Fr>::default();
    let expr = "(hide 'x 456)";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_hide_secret() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret (hide 123 456))";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 5, None);
}

#[test]
fn test_prove_hide_open_sym() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (hide 123 'x))";
    let x = s.user_sym("x");
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 5, None);
}

#[test]
fn test_prove_commit_open_sym() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (commit 'x))";
    let x = s.user_sym("x");
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 4, None);
}

#[test]
fn test_prove_commit_open() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (commit 123))";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 4, None);
}

#[test]
fn test_prove_commit_error() {
    let s = &mut Store::<Fr>::default();
    let expr = "(commit 123 456)";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_open_error() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open 123 456)";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 1, None);
}

#[test]
fn test_prove_open_wrong_type() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open 'asdf)";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_secret_wrong_type() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret 'asdf)";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_commit_secret() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret (commit 123))";
    let expected = s.num(0);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 4, None);
}

#[test]
fn test_prove_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(num 123)";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 2, None);
}

#[test]
fn test_prove_num_char() {
    let s = &mut Store::<Fr>::default();
    let expr = r"(num #\a)";
    let expected = s.num(97);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 2, None);
}

#[test]
fn test_prove_char_num() {
    let s = &mut Store::<Fr>::default();
    let expr = r#"(char 97)"#;
    let expected_a = s.read(r"#\a").unwrap();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        expr,
        Some(expected_a),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
}

#[test]
fn test_prove_char_coercion() {
    let s = &mut Store::<Fr>::default();
    let expr = r#"(char (- 0 4294967200))"#;
    let expr2 = r#"(char (- 0 4294967199))"#;
    let expected_a = s.read(r"#\a").unwrap();
    let expected_b = s.read(r"#\b").unwrap();
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        expr,
        Some(expected_a),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        expr2,
        Some(expected_b),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}

#[test]
fn test_prove_commit_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(num (commit 123))";
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 4, None);
}

#[test]
fn test_prove_hide_open_comm_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (comm (num (hide 123 456))))";
    let expected = s.num(456);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 9, None);
}

#[test]
fn test_prove_hide_secret_comm_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret (comm (num (hide 123 456))))";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 9, None);
}

#[test]
fn test_prove_commit_open_comm_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (comm (num (commit 123))))";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 8, None);
}

#[test]
fn test_prove_commit_secret_comm_num() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret (comm (num (commit 123))))";
    let expected = s.num(0);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 8, None);
}

#[test]
fn test_prove_commit_num_open() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open (num (commit 123)))";
    let expected = s.num(123);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 6, None);
}

#[test]
fn test_prove_num_invalid_tag() {
    let s = &mut Store::<Fr>::default();
    let expr = "(num (quote x))";
    let expr1 = "(num \"asdf\")";
    let expr2 = "(num '(1))";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_comm_invalid_tag() {
    let s = &mut Store::<Fr>::default();
    let expr = "(comm (quote x))";
    let expr1 = "(comm \"asdf\")";
    let expr2 = "(comm '(1))";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_char_invalid_tag() {
    let s = &mut Store::<Fr>::default();
    let expr = "(char (quote x))";
    let expr1 = "(char \"asdf\")";
    let expr2 = "(char '(1))";
    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
}

#[test]
fn test_prove_terminal_sym() {
    let s = &mut Store::<Fr>::default();
    let expr = "(quote x)";
    let x = s.user_sym("x");
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 1, None);
}

#[test]
#[should_panic = "hidden value could not be opened"]
fn test_prove_open_opaque_commit() {
    let s = &mut Store::<Fr>::default();
    let expr = "(open 123)";
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
}

#[test]
#[should_panic]
fn test_prove_secret_invalid_tag() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret 123)";
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
}

#[test]
#[should_panic = "secret could not be extracted"]
fn test_prove_secret_opaque_commit() {
    let s = &mut Store::<Fr>::default();
    let expr = "(secret (comm 123))";
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
}

#[test]
fn test_str_car_cdr_cons() {
    let s = &mut Store::<Fr>::default();
    let a = s.read(r"#\a").unwrap();
    let apple = s.read(r#" "apple" "#).unwrap();
    let a_pple = s.read(r#" (#\a . "pple") "#).unwrap();
    let pple = s.read(r#" "pple" "#).unwrap();
    let empty = s.intern_string("");
    let nil = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car "apple")"#,
        Some(a),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr "apple")"#,
        Some(pple),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(car "")"#,
        Some(nil),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cdr "")"#,
        Some(empty),
        None,
        Some(terminal),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(cons #\a "pple")"#,
        Some(a_pple),
        None,
        Some(terminal),
        None,
        3,
        None,
    );

    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(strcons #\a "pple")"#,
        Some(apple),
        None,
        Some(terminal),
        None,
        3,
        None,
    );

    test_aux::<_, _, M1<'_, _>>(
        s,
        r"(strcons #\a #\b)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );

    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(strcons "a" "b")"#,
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );

    test_aux::<_, _, M1<'_, _>>(
        s,
        r#"(strcons 1 2)"#,
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
}

fn relational_aux(s: &mut Store<Fr>, op: &str, a: &str, b: &str, res: bool) {
    let expr = &format!("({op} {a} {b})");
    let expected = if res {
        lurk_sym_ptr!(s, t)
    } else {
        lurk_sym_ptr!(s, nil)
    };
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 3, None);
}

#[ignore]
#[test]
fn test_prove_test_relational() {
    let s = &mut Store::<Fr>::default();
    let lt = "<";
    let gt = ">";
    let lte = "<=";
    let gte = ">=";
    let zero = "0";
    let one = "1";
    let two = "2";

    let most_negative = &format!("{}", Num::<Fr>::most_negative());
    let most_positive = &format!("{}", Num::<Fr>::most_positive());
    let neg_one = &format!("{}", Num::<Fr>::Scalar(Fr::zero() - Fr::one()));

    relational_aux(s, lt, one, two, true);
    relational_aux(s, gt, one, two, false);
    relational_aux(s, lte, one, two, true);
    relational_aux(s, gte, one, two, false);

    relational_aux(s, lt, two, one, false);
    relational_aux(s, gt, two, one, true);
    relational_aux(s, lte, two, one, false);
    relational_aux(s, gte, two, one, true);

    relational_aux(s, lt, one, one, false);
    relational_aux(s, gt, one, one, false);
    relational_aux(s, lte, one, one, true);
    relational_aux(s, gte, one, one, true);

    relational_aux(s, lt, zero, two, true);
    relational_aux(s, gt, zero, two, false);
    relational_aux(s, lte, zero, two, true);
    relational_aux(s, gte, zero, two, false);

    relational_aux(s, lt, two, zero, false);
    relational_aux(s, gt, two, zero, true);
    relational_aux(s, lte, two, zero, false);
    relational_aux(s, gte, two, zero, true);

    relational_aux(s, lt, zero, zero, false);
    relational_aux(s, gt, zero, zero, false);
    relational_aux(s, lte, zero, zero, true);
    relational_aux(s, gte, zero, zero, true);

    relational_aux(s, lt, most_negative, zero, true);
    relational_aux(s, gt, most_negative, zero, false);
    relational_aux(s, lte, most_negative, zero, true);
    relational_aux(s, gte, most_negative, zero, false);

    relational_aux(s, lt, zero, most_negative, false);
    relational_aux(s, gt, zero, most_negative, true);
    relational_aux(s, lte, zero, most_negative, false);
    relational_aux(s, gte, zero, most_negative, true);

    relational_aux(s, lt, most_negative, most_positive, true);
    relational_aux(s, gt, most_negative, most_positive, false);
    relational_aux(s, lte, most_negative, most_positive, true);
    relational_aux(s, gte, most_negative, most_positive, false);

    relational_aux(s, lt, most_positive, most_negative, false);
    relational_aux(s, gt, most_positive, most_negative, true);
    relational_aux(s, lte, most_positive, most_negative, false);
    relational_aux(s, gte, most_positive, most_negative, true);

    relational_aux(s, lt, most_negative, most_negative, false);
    relational_aux(s, gt, most_negative, most_negative, false);
    relational_aux(s, lte, most_negative, most_negative, true);
    relational_aux(s, gte, most_negative, most_negative, true);

    relational_aux(s, lt, one, most_positive, true);
    relational_aux(s, gt, one, most_positive, false);
    relational_aux(s, lte, one, most_positive, true);
    relational_aux(s, gte, one, most_positive, false);

    relational_aux(s, lt, most_positive, one, false);
    relational_aux(s, gt, most_positive, one, true);
    relational_aux(s, lte, most_positive, one, false);
    relational_aux(s, gte, most_positive, one, true);

    relational_aux(s, lt, one, most_negative, false);
    relational_aux(s, gt, one, most_negative, true);
    relational_aux(s, lte, one, most_negative, false);
    relational_aux(s, gte, one, most_negative, true);

    relational_aux(s, lt, most_negative, one, true);
    relational_aux(s, gt, most_negative, one, false);
    relational_aux(s, lte, most_negative, one, true);
    relational_aux(s, gte, most_negative, one, false);

    relational_aux(s, lt, neg_one, most_positive, true);
    relational_aux(s, gt, neg_one, most_positive, false);
    relational_aux(s, lte, neg_one, most_positive, true);
    relational_aux(s, gte, neg_one, most_positive, false);

    relational_aux(s, lt, most_positive, neg_one, false);
    relational_aux(s, gt, most_positive, neg_one, true);
    relational_aux(s, lte, most_positive, neg_one, false);
    relational_aux(s, gte, most_positive, neg_one, true);

    relational_aux(s, lt, neg_one, most_negative, false);
    relational_aux(s, gt, neg_one, most_negative, true);
    relational_aux(s, lte, neg_one, most_negative, false);
    relational_aux(s, gte, neg_one, most_negative, true);

    relational_aux(s, lt, most_negative, neg_one, true);
    relational_aux(s, gt, most_negative, neg_one, false);
    relational_aux(s, lte, most_negative, neg_one, true);
    relational_aux(s, gte, most_negative, neg_one, false);
}

#[test]
fn test_relational_edge_case_identity() {
    let s = &mut Store::<Fr>::default();
    // Normally, a value cannot be less than the result of incrementing it.
    // However, the most positive field element (when viewed as signed)
    // is the exception. Incrementing it yields the most negative element,
    // which is less than the most positive.
    let expr = "(let ((most-positive (/ (- 0 1) 2))
                        (most-negative (+ 1 most-positive)))
                    (< most-negative most-positive))";
    let t = lurk_sym_ptr!(s, t);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 19, None);
}

#[test]
fn test_prove_test_eval() {
    let s = &mut Store::<Fr>::default();
    let expr = "(* 3 (eval  (cons '+ (cons 1 (cons 2 nil)))))";
    let expr2 = "(* 5 (eval '(+ 1 a) '((a . 3))))"; // two-arg eval, optional second arg is env.
    let res = s.num(9);
    let res2 = s.num(20);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 17, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 9, None);
}

#[test]
fn test_prove_test_keyword() {
    let s = &mut Store::<Fr>::default();

    let expr = ":asdf";
    let expr2 = "(eq :asdf :asdf)";
    let expr3 = "(eq :asdf 'asdf)";
    let res = s.key("asdf");
    let res2 = lurk_sym_ptr!(s, t);
    let res3 = lurk_sym_ptr!(s, nil);

    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 1, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res3), None, Some(terminal), None, 3, None);
}

// The following functional commitment tests were discovered to fail. They are commented out (as tests) for now so
// they can be addressed independently in future work.

#[test]
fn test_prove_functional_commitment() {
    let s = &mut Store::<Fr>::default();

    let expr = "(let ((f (commit (let ((num 9)) (lambda (f) (f num)))))
                        (inc (lambda (x) (+ x 1))))
                    ((open f) inc))";
    let res = s.num(10);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 25, None);
}

#[test]
#[ignore]
fn test_prove_complicated_functional_commitment() {
    let s = &mut Store::<Fr>::default();

    let expr = "(let ((f (commit (let ((nums '(1 2 3))) (lambda (f) (f nums)))))
                        (in (letrec ((sum-aux (lambda (acc nums)
                                            (if nums
                                            (sum-aux (+ acc (car nums)) (cdr nums))
                                            acc)))
                                (sum (sum-aux 0)))
                            (lambda (nums)
                            (sum nums)))))

                    ((open f) in))";
    let res = s.num(6);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 108, None);
}

#[test]
fn test_prove_test_fold_cons_regression() {
    let s = &mut Store::<Fr>::default();
    let expr = "(letrec ((fold (lambda (op acc l)
                                    (if l
                                        (fold op (op acc (car l)) (cdr l))
                                        acc))))
                    (fold (lambda (x y) (+ x y)) 0 '(1 2 3)))";
    let res = s.num(6);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 152, None);
}

#[test]
fn test_prove_test_lambda_args_regression() {
    let s = &mut Store::<Fr>::default();

    let expr = "(cons (lambda (x y) nil) nil)";
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_reduce_sym_contradiction_regression() {
    let s = &mut Store::<Fr>::default();

    let expr = "(eval 'a '(nil))";
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 4, None);
}

#[test]
fn test_prove_test_self_eval_env_not_nil() {
    let s = &mut Store::<Fr>::default();

    // NOTE: cond1 shouldn't depend on env-is-not-nil
    // therefore this unit test is not very useful
    // the conclusion is that by removing condition env-is-not-nil from cond1,
    // we solve this soundness problem
    // this solution makes the circuit a bit smaller
    let expr = "(let ((a 1)) t)";

    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_test_self_eval_nil() {
    let s = &mut Store::<Fr>::default();

    // nil doesn't have SYM tag
    let expr = "nil";

    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 1, None);
}

#[test]
fn test_prove_test_env_not_nil_and_binding_nil() {
    let s = &mut Store::<Fr>::default();

    let expr = "(let ((a 1) (b 2)) c)";

    let error = s.get_cont_error();
    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 7, None);
}

#[test]
fn test_prove_test_eval_bad_form() {
    let s = &mut Store::<Fr>::default();
    let expr = "(* 5 (eval '(+ 1 a) '((0 . 3))))"; // two-arg eval, optional second arg is env. This tests for error on malformed env.
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 8, None);
}

#[test]
fn test_prove_test_u64_self_evaluating() {
    let s = &mut Store::<Fr>::default();

    let expr = "123u64";
    let res = s.uint64(123);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 1, None);
}

#[test]
fn test_prove_test_u64_mul() {
    let s = &mut Store::<Fr>::default();

    let expr = "(* (u64 18446744073709551615) (u64 2))";
    let expr2 = "(* 18446744073709551615u64 2u64)";
    let expr3 = "(* (- 0u64 1u64) 2u64)";
    let expr4 = "(u64 18446744073709551617)";
    let res = s.uint64(18446744073709551614);
    let res2 = s.uint64(1);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 7, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res), None, Some(terminal), None, 6, None);
    test_aux::<_, _, M1<'_, _>>(s, expr4, Some(res2), None, Some(terminal), None, 2, None);
}

#[test]
fn test_prove_test_u64_add() {
    let s = &mut Store::<Fr>::default();

    let expr = "(+ 18446744073709551615u64 2u64)";
    let expr2 = "(+ (- 0u64 1u64) 2u64)";
    let res = s.uint64(1);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 6, None);
}

#[test]
fn test_prove_test_u64_sub() {
    let s = &mut Store::<Fr>::default();

    let expr = "(- 2u64 1u64)";
    let expr2 = "(- 0u64 1u64)";
    let expr3 = "(+ 1u64 (- 0u64 1u64))";
    let res = s.uint64(1);
    let res2 = s.uint64(18446744073709551615);
    let res3 = s.uint64(0);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res3), None, Some(terminal), None, 6, None);
}

#[test]
fn test_prove_test_u64_div() {
    let s = &mut Store::<Fr>::default();

    let expr = "(/ 100u64 2u64)";
    let res = s.uint64(50);

    let expr2 = "(/ 100u64 3u64)";
    let res2 = s.uint64(33);

    let expr3 = "(/ 100u64 0u64)";

    let terminal = s.get_cont_terminal();
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_test_u64_mod() {
    let s = &mut Store::<Fr>::default();

    let expr = "(% 100u64 2u64)";
    let res = s.uint64(0);

    let expr2 = "(% 100u64 3u64)";
    let res2 = s.uint64(1);

    let expr3 = "(% 100u64 0u64)";

    let terminal = s.get_cont_terminal();
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_test_num_mod() {
    let s = &mut Store::<Fr>::default();

    let expr = "(% 100 3)";
    let expr2 = "(% 100 3u64)";
    let expr3 = "(% 100u64 3)";

    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_test_u64_comp() {
    let s = &mut Store::<Fr>::default();

    let expr = "(< 0u64 1u64)";
    let expr2 = "(< 1u64 0u64)";
    let expr3 = "(<= 0u64 1u64)";
    let expr4 = "(<= 1u64 0u64)";

    let expr5 = "(> 0u64 1u64)";
    let expr6 = "(> 1u64 0u64)";
    let expr7 = "(>= 0u64 1u64)";
    let expr8 = "(>= 1u64 0u64)";

    let expr9 = "(<= 0u64 0u64)";
    let expr10 = "(>= 0u64 0u64)";

    let t = lurk_sym_ptr!(s, t);
    let nil = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(nil), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, Some(t), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr4, Some(nil), None, Some(terminal), None, 3, None);

    test_aux::<_, _, M1<'_, _>>(s, expr5, Some(nil), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr6, Some(t), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr7, Some(nil), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr8, Some(t), None, Some(terminal), None, 3, None);

    test_aux::<_, _, M1<'_, _>>(s, expr9, Some(t), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr10, Some(t), None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_test_u64_conversion() {
    let s = &mut Store::<Fr>::default();

    let expr = "(+ 0 1u64)";
    let expr2 = "(num 1u64)";
    let expr3 = "(+ 1 1u64)";
    let expr4 = "(u64 (+ 1 1))";
    let res = s.intern_num(1);
    let res2 = s.intern_num(2);
    let res3 = s.intern_u64(2);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res2), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr4, Some(res3), None, Some(terminal), None, 5, None);
}

#[test]
fn test_prove_test_u64_num_comparison() {
    let s = &mut Store::<Fr>::default();

    let expr = "(= 1 1u64)";
    let expr2 = "(= 1 2u64)";
    let t = lurk_sym_ptr!(s, t);
    let nil = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(nil), None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_test_u64_num_cons() {
    let s = &mut Store::<Fr>::default();

    let expr = "(cons 1 1u64)";
    let expr2 = "(cons 1u64 1)";
    let res = s.read("(1 . 1u64)").unwrap();
    let res2 = s.read("(1u64 . 1)").unwrap();
    let terminal = s.get_cont_terminal();

    test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
    test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
}

#[test]
fn test_prove_test_hide_u64_secret() {
    let s = &mut Store::<Fr>::default();

    let expr = "(hide 0u64 123)";
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_test_mod_by_zero_error() {
    let s = &mut Store::<Fr>::default();

    let expr = "(% 0 0)";
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_dotted_syntax_error() {
    let s = &mut Store::<Fr>::default();
    let expr = "(let ((a (lambda (x) (+ x 1)))) (a . 1))";
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
}

#[test]
fn test_prove_call_literal_fun() {
    let s = &mut Store::<Fr>::default();
    let empty_env = lurk_sym_ptr!(s, nil);
    let arg = s.user_sym("x");
    let body = s.read("((+ x 1))").unwrap();
    let fun = s.intern_fun(arg, body, empty_env);
    let input = s.num(9);
    let expr = s.list(&[fun, input]);
    let res = s.num(10);
    let terminal = s.get_cont_terminal();
    let lang: Arc<Lang<Fr, Coproc<Fr>>> = Arc::new(Lang::new());

    nova_test_full_aux2::<_, _, M1<'_, _>>(
        s,
        expr,
        Some(res),
        None,
        Some(terminal),
        None,
        7,
        DEFAULT_REDUCTION_COUNT,
        false,
        None,
        lang,
    );
}

#[test]
fn test_prove_lambda_body_syntax() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();

    test_aux::<_, _, M1<'_, _>>(s, "((lambda ()))", None, None, Some(error), None, 2, None);
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda () 1 2))",
        None,
        None,
        Some(error),
        None,
        2,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x)) 1)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x) 1 2) 1)",
        None,
        None,
        Some(error),
        None,
        3,
        None,
    );
}

#[test]
#[ignore]
fn test_prove_non_symbol_binding_error() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();

    let test = |x| {
        let expr = format!("(let (({x} 123)) {x})");
        let expr2 = format!("(letrec (({x} 123)) {x})");
        let expr3 = format!("(lambda ({x}) {x})");

        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        test_aux::<_, _, M1<'_, _>>(s, &expr2, None, None, Some(error), None, 1, None);
        test_aux::<_, _, M1<'_, _>>(s, &expr3, None, None, Some(error), None, 1, None);
    };

    test(":a");
    test("1");
    test("\"string\"");
    test("1u64");
    test("#\\x");
}

#[test]
fn test_prove_head_with_sym_mimicking_value() {
    let s = &mut Store::<Fr>::default();
    let error = s.get_cont_error();

    let hash_num = |s: &Store<Fr>, state: Rc<RefCell<State>>, name| {
        let sym = s.read_with_state(state, name).unwrap();
        let z_ptr = s.hash_expr(&sym).unwrap();
        let hash = *z_ptr.value();
        Num::Scalar(hash)
    };

    let state = State::init_lurk_state().rccell();
    {
        // binop
        let expr = format!("({} 1 1)", hash_num(s, state.clone(), "+"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // unop
        let expr = format!("({} '(1 . 2))", hash_num(s, state.clone(), "car"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // let_or_letrec
        let expr = format!("({} ((a 1)) a)", hash_num(s, state.clone(), "let"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // current-env
        let expr = format!("({})", hash_num(s, state.clone(), "current-env"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // lambda
        let expr = format!("({} (x) 123)", hash_num(s, state.clone(), "lambda"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // quote
        let expr = format!("({} asdf)", hash_num(s, state.clone(), "quote"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
    {
        // if
        let expr = format!("({} t 123 456)", hash_num(s, state, "if"));
        test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
    }
}

#[test]
#[traced_test]
fn test_dumb_lang() {
    use crate::coprocessor::test::DumbCoprocessor;
    use crate::eval::tests::coproc::DumbCoproc;

    let s = &mut Store::<Fr>::new();

    let mut lang = Lang::<Fr, DumbCoproc<Fr>>::new();
    let name = user_sym("cproc-dumb");
    let dumb = DumbCoprocessor::new();
    let coproc = DumbCoproc::DC(dumb);

    lang.add_coprocessor(name, coproc, s);

    // 9^2 + 8 = 89
    let expr = "(cproc-dumb 9 8)";

    // The dumb coprocessor cannot be shadowed.
    let expr2 = "(let ((cproc-dumb (lambda (a b) (* a b))))
                (cproc-dumb 9 8))";

    let expr3 = "(cproc-dumb 9 8 123)";
    let expr4 = "(cproc-dumb 9)";

    let res = s.num(89);
    let error = s.get_cont_error();
    let lang = Arc::new(lang);

    test_aux::<_, _, C1Lurk<'_, _, DumbCoproc<_>>>(
        s,
        expr,
        Some(res),
        None,
        None,
        None,
        2,
        Some(lang.clone()),
    );
    test_aux::<_, _, C1Lurk<'_, _, DumbCoproc<_>>>(
        s,
        expr2,
        Some(res),
        None,
        None,
        None,
        4,
        Some(lang.clone()),
    );
    test_aux::<_, _, C1Lurk<'_, _, DumbCoproc<_>>>(
        s,
        expr3,
        None,
        None,
        Some(error),
        None,
        1,
        Some(lang.clone()),
    );
    test_aux::<_, _, C1Lurk<'_, _, DumbCoproc<_>>>(
        s,
        expr4,
        None,
        None,
        Some(error),
        None,
        1,
        Some(lang),
    );
}

// This is related to issue #426
#[test]
fn test_prove_lambda_body_nil() {
    let s = &mut Store::<Fr>::default();
    let expected = lurk_sym_ptr!(s, nil);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "((lambda (x) nil) 0)",
        Some(expected),
        None,
        Some(terminal),
        None,
        4,
        None,
    );
}

// The following 3 tests are related to issue #424
#[test]
fn test_letrec_let_nesting() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(2);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((x (let ((z 0)) 1))) 2)",
        Some(expected),
        None,
        Some(terminal),
        None,
        6,
        None,
    );
}
#[test]
fn test_let_sequencing() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(1);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(let ((x 0) (y x)) 1)",
        Some(expected),
        None,
        Some(terminal),
        None,
        5,
        None,
    );
}
#[test]
fn test_letrec_sequencing() {
    let s = &mut Store::<Fr>::default();
    let expected = s.num(3);
    let terminal = s.get_cont_terminal();
    test_aux::<_, _, M1<'_, _>>(
        s,
        "(letrec ((x 0) (y (letrec ((inner 1)) 2))) 3)",
        Some(expected),
        None,
        Some(terminal),
        None,
        8,
        None,
    );
}

use lurk_macros::generic_tests;

// Most of these are actually evaluation interfaces! TODO: Make it so
pub trait MultiFrameExt<'a, F: LurkField, C: Coprocessor<F> + 'a>:
    MultiFrameTrait<'a, F, C>
{
    fn ptr_for_num(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
        u: u64,
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;
    fn ptr_for_u64(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
        u: u64,
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;
    fn ptr_for_true(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;
    fn ptr_for_nil(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;

    fn ptr_for_list(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
        xs: &[<Self as MultiFrameTrait<'a, F, C>>::Ptr],
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;

    fn ptr_for_fun(
        s: &<Self as MultiFrameTrait<'a, F, C>>::Store,
        arg: <Self as MultiFrameTrait<'a, F, C>>::Ptr,
        body: <Self as MultiFrameTrait<'a, F, C>>::Ptr,
        env: <Self as MultiFrameTrait<'a, F, C>>::Ptr,
    ) -> <Self as MultiFrameTrait<'a, F, C>>::Ptr;
}

impl<'a, F: LurkField> MultiFrameExt<'a, F, Coproc<F>> for C1Lurk<'a, F, Coproc<F>> {
    fn ptr_for_num(s: &crate::store::Store<F>, u: u64) -> crate::ptr::Ptr<F> {
        s.num(u)
    }

    fn ptr_for_u64(s: &crate::store::Store<F>, u: u64) -> crate::ptr::Ptr<F> {
        s.uint64(u)
    }

    fn ptr_for_true(s: &crate::store::Store<F>) -> crate::ptr::Ptr<F> {
        lurk_sym_ptr!(s, t)
    }

    fn ptr_for_nil(s: &crate::store::Store<F>) -> crate::ptr::Ptr<F> {
        lurk_sym_ptr!(s, nil)
    }

    fn ptr_for_list(s: &crate::store::Store<F>, xs: &[crate::ptr::Ptr<F>]) -> crate::ptr::Ptr<F> {
        s.list(xs)
    }

    fn ptr_for_fun(
        s: &crate::store::Store<F>,
        arg: crate::ptr::Ptr<F>,
        body: crate::ptr::Ptr<F>,
        env: crate::ptr::Ptr<F>,
    ) -> crate::ptr::Ptr<F> {
        s.intern_fun(arg, body, env)
    }
}

impl<'a, F: LurkField> MultiFrameExt<'a, F, Coproc<F>> for C1LEM<'a, F, Coproc<F>> {
    fn ptr_for_num(_s: &crate::lem::store::Store<F>, u: u64) -> crate::lem::pointers::Ptr<F> {
        crate::lem::pointers::Ptr::num_u64(u)
    }

    fn ptr_for_u64(_s: &crate::lem::store::Store<F>, u: u64) -> crate::lem::pointers::Ptr<F> {
        crate::lem::pointers::Ptr::u64(u)
    }

    fn ptr_for_true(s: &crate::lem::store::Store<F>) -> crate::lem::pointers::Ptr<F> {
        s.intern_lurk_symbol("t")
    }

    fn ptr_for_nil(s: &crate::lem::store::Store<F>) -> crate::lem::pointers::Ptr<F> {
        s.intern_nil()
    }

    fn ptr_for_list(
        s: &crate::lem::store::Store<F>,
        xs: &[crate::lem::pointers::Ptr<F>],
    ) -> crate::lem::pointers::Ptr<F> {
        s.list(xs.to_vec()) // admissible for small tests
    }

    fn ptr_for_fun(
        s: &crate::lem::store::Store<F>,
        arg: crate::lem::pointers::Ptr<F>,
        body: crate::lem::pointers::Ptr<F>,
        env: crate::lem::pointers::Ptr<F>,
    ) -> crate::lem::pointers::Ptr<F> {
        s.intern_3_ptrs(
            crate::lem::Tag::Expr(crate::tag::ExprTag::Fun),
            arg,
            body,
            env,
        )
    }
}

#[generic_tests]
pub mod proof_tests {
    use crate::num::Num;
    use crate::proof::nova::{CurveCycleEquipped, G1, G2};
    use crate::proof::{Coprocessor, EvaluationStore, MultiFrameTrait};
    use abomonation::Abomonation;
    use nova::traits::Group;

    use super::MultiFrameExt;
    use super::{nova_test_full_aux, nova_test_full_aux2, test_aux, DEFAULT_REDUCTION_COUNT};
    use std::sync::Mutex;

    trait Any {}
    impl<T> Any for T {}

    struct Warehouse {
        ts: Mutex<Vec<Box<dyn Any>>>,
    }

    // We reproduce those marker traits because of the same requirement on store
    unsafe impl Send for Warehouse {}
    unsafe impl Sync for Warehouse {}

    impl Warehouse {
        fn store<T: Send + Sync>(&self, t: T) -> &T {
            let mut ts = self.ts.lock().unwrap();
            let boxed = Box::new(t);
            let ptr = &*boxed as *const T;
            let any = unsafe {
                // A combination of lifetime elision and implied lifetime bounds
                // ensures that any borrowed data in T outlives self.
                // Written explicitly, the store signature is:
                // fn store<'a, T: 'a>(&'a self, t: T) -> &'a T.
                // That means we can treat the borrowed data in T as being alive as
                // long as we need.
                std::mem::transmute::<Box<dyn Any>, Box<dyn Any + 'static>>(boxed)
            };
            ts.push(any);
            unsafe { &*ptr }
        }
    }

    static WAREHOUSE: Warehouse = Warehouse {
        ts: Mutex::new(Vec::new()),
    };

    #[test]
    pub fn test_prove_binop<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&store, 3);
        let terminal = store.get_cont_terminal();
        // we need to create a reference to the store that outlives the outer 'a lifetime,
        // and this requires the store to be "kept" elsewhere. This is a hack.
        let s_ref: &'a _ = WAREHOUSE.store(store);

        test_aux::<_, _, G>(
            s_ref,
            "(+ 1 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[should_panic]
    // This tests the testing mechanism. Since the supplied expected value is wrong,
    // the test should panic on an assertion failure.
    pub fn test_prove_binop_fail<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&store, 2);
        let terminal = store.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(store);
        test_aux::<_, _, G>(
            s_ref,
            "(+ 1 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_arithmetic_let<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&store, 3);
        let terminal = store.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(store);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 5)
                      (b 1)
                      (c 2))
                 (/ (+ a b) c))",
            Some(expected),
            None,
            Some(terminal),
            None,
            18,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_eq<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_true(&store);
        let terminal = store.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(store);
        nova_test_full_aux::<_, _, G>(
            s_ref,
            "(eq 5 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            DEFAULT_REDUCTION_COUNT,
            true,
            None,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_num_equal<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_true(&store);
        let nil: <G as MultiFrameTrait<F, C>>::Ptr = G::ptr_for_nil(&store);
        let terminal = store.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(store);
        test_aux::<_, _, G>(
            s_ref,
            "(= 5 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "(= 5 6)",
            Some(nil),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_invalid_num_equal<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let nil = G::ptr_for_nil(&store);
        let num_5 = G::ptr_for_num(&store, 5);
        let error = store.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(store);

        test_aux::<_, _, G>(
            s_ref,
            "(= 5 nil)",
            Some(nil),
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "(= nil 5)",
            Some(num_5),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_equal<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let nil = G::ptr_for_nil(&store);
        let t = G::ptr_for_true(&store);
        let terminal = store.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(store);

        test_aux::<_, _, G>(
            s_ref,
            "(eq 5 nil)",
            Some(nil),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, G>(
            s_ref,
            "(eq nil 5)",
            Some(nil),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, G>(
            s_ref,
            "(eq nil nil)",
            Some(t),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, G>(
            s_ref,
            "(eq 5 5)",
            Some(t),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_quote_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let store = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = store.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(store);

        test_aux::<_, _, G>(
            s_ref,
            "(quote (1) (2))",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_if<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_5 = G::ptr_for_num(&s, 5);
        let num_6 = G::ptr_for_num(&s, 6);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(if t 5 6)",
            Some(num_5),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "(if nil 5 6)",
            Some(num_6),
            None,
            Some(terminal),
            None,
            3,
            None,
        )
    }

    #[test]
    pub fn test_prove_if_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_5 = G::ptr_for_num(&s, 5);
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(if nil 5 6 7)",
            Some(num_5),
            None,
            Some(error),
            None,
            2,
            None,
        )
    }

    #[test]
    #[ignore]
    pub fn test_prove_if_fully_evaluates<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 10);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(if t (+ 5 5) 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    #[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
    pub fn test_prove_recursion1<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base)
                               (lambda (exponent)
                                 (if (= 0 exponent)
                                     1
                                     (* base ((exp base) (- exponent 1))))))))
                 ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            66,
            None,
        );
    }

    #[test]
    #[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
    pub fn test_prove_recursion2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base)
                                  (lambda (exponent)
                                     (lambda (acc)
                                       (if (= 0 exponent)
                                          acc
                                          (((exp base) (- exponent 1)) (* acc base))))))))
                (((exp 5) 2) 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            93,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_unop_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        // We need to at least use chunk size 1 to exercise the regression.
        // Also use a non-1 value to check the MultiFrame case.
        for chunk_count in 1..2 {
            let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
            let expected = G::ptr_for_true(&s);
            let num_1 = G::ptr_for_num(&s, 1);
            let num_2 = G::ptr_for_num(&s, 2);
            let num_123 = G::ptr_for_num(&s, 123);
            let terminal = s.get_cont_terminal();
            let s_ref: &'a _ = WAREHOUSE.store(s);

            nova_test_full_aux::<_, _, G>(
                s_ref,
                "(atom 123)",
                Some(expected),
                None,
                Some(terminal),
                None,
                2,
                chunk_count, // This needs to be 1 to exercise the bug.
                false,
                None,
                None,
            );

            nova_test_full_aux::<_, _, G>(
                s_ref,
                "(car '(1 . 2))",
                Some(num_1),
                None,
                Some(terminal),
                None,
                2,
                chunk_count, // This needs to be 1 to exercise the bug.
                false,
                None,
                None,
            );

            nova_test_full_aux::<_, _, G>(
                s_ref,
                "(cdr '(1 . 2))",
                Some(num_2),
                None,
                Some(terminal),
                None,
                2,
                chunk_count, // This needs to be 1 to exercise the bug.
                false,
                None,
                None,
            );

            nova_test_full_aux::<_, _, G>(
                s_ref,
                "(emit 123)",
                Some(num_123),
                None,
                Some(terminal),
                None,
                3,
                chunk_count,
                false,
                None,
                None,
            )
        }
    }

    #[test]
    #[ignore]
    pub fn test_prove_emit_output<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_123 = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(emit 123)",
            Some(num_123),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_99 = G::ptr_for_num(&s, 99);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (x) x) 99)",
            Some(num_99),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_99 = G::ptr_for_num(&s, 99);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (y)
                    ((lambda (x) y) 888))
                  99)",
            Some(num_99),
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate3<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_999 = G::ptr_for_num(&s, 999);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (y)
                     ((lambda (x)
                        ((lambda (z) z)
                         x))
                      y))
                   999)",
            Some(num_999),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate4<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_888 = G::ptr_for_num(&s, 888);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (y)
                     ((lambda (x)
                        ((lambda (z) z)
                         x))
                      ;; NOTE: We pass a different value here.
                      888))
                  999)",
            Some(num_888),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate5<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_999 = G::ptr_for_num(&s, 999);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(((lambda (fn)
                      (lambda (x) (fn x)))
                    (lambda (y) y))
                   999)",
            Some(num_999),
            None,
            Some(terminal),
            None,
            13,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_evaluate_sum<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_9 = G::ptr_for_num(&s, 9);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(+ 2 (+ 3 4))",
            Some(num_9),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }

    #[test]
    pub fn test_prove_binop_rest_is_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let num_9 = G::ptr_for_num(&s, 9);
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(- 9 8 7)",
            Some(num_9),
            None,
            Some(error),
            None,
            2,
            None,
        );
        test_aux::<_, _, G>(
            s_ref,
            "(= 9 8 7)",
            Some(num_9),
            None,
            Some(error),
            None,
            2,
            None,
        );
    }

    fn op_syntax_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
        T: crate::tag::Op + Copy,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        for op in T::all() {
            let name = op.symbol_name();

            if !op.supports_arity(0) {
                let expr = format!("({name})");
                tracing::debug!("{:?}", &expr);

                test_aux::<_, _, G>(s_ref, &expr, None, None, Some(error), None, 1, None);
            }
            if !op.supports_arity(1) {
                let expr = format!("({name} 123)");
                tracing::debug!("{:?}", &expr);

                test_aux::<_, _, G>(s_ref, &expr, None, None, Some(error), None, 1, None);
            }
            if !op.supports_arity(2) {
                let expr = format!("({name} 123 456)");
                tracing::debug!("{:?}", &expr);

                test_aux::<_, _, G>(s_ref, &expr, None, None, Some(error), None, 1, None);
            }

            if !op.supports_arity(3) {
                let expr = format!("({name} 123 456 789)");
                tracing::debug!("{:?}", &expr);
                let iterations = if op.supports_arity(2) { 2 } else { 1 };

                test_aux::<_, _, G>(
                    s_ref,
                    &expr,
                    None,
                    None,
                    Some(error),
                    None,
                    iterations,
                    None,
                );
            }
        }
    }

    #[test]
    #[ignore]
    pub fn test_prove_unop_syntax_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        op_syntax_error::<'a, F, C, G, crate::tag::Op1>();
    }

    #[test]
    #[ignore]
    pub fn test_prove_binop_syntax_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        op_syntax_error::<'a, F, C, G, crate::tag::Op2>();
    }

    #[test]
    pub fn test_prove_diff<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 4);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(- 9 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_product<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 45);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(* 9 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_quotient<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 7);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(/ 21 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_error_div_by_zero<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 0);
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(/ 21 0)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_error_invalid_type_and_not_cons<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(/ 21 nil)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_adder<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 5);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(((lambda (x)
                (lambda (y)
                    (+ x y)))
                2)
                3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            13,
            None,
        );
    }

    #[test]
    pub fn test_prove_current_env_simple<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(current-env)",
            Some(expected),
            None,
            Some(terminal),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_current_env_rest_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = s.read("(current-env a)").unwrap();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(current-env a)",
            Some(expected),
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_let_simple<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 1);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 1))
                a)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_let_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 1 2)) a)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_letrec_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((a 1 2)) a)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_lambda_empty_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (x)) 0)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_let_empty_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, "(let)", None, None, Some(error), None, 1, None);
    }

    #[test]
    pub fn test_prove_let_empty_body_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 1)))",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_letrec_empty_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, "(letrec)", None, None, Some(error), None, 1, None);
    }

    #[test]
    pub fn test_prove_letrec_empty_body_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((a 1)))",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_let_body_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_true(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(eq nil (let () nil))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    pub fn test_prove_let_rest_body_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 1)) a 1)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_letrec_rest_body_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((a 1)) a 1)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_let_null_bindings<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 3);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let () (+ 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }
    #[test]
    #[ignore]
    pub fn test_prove_letrec_null_bindings<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 3);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec () (+ 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_let<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 6);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((a 1)
                    (b 2)
                    (c 3))
                (+ a (+ b c)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            18,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_arithmetic<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 20);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((((lambda (x)
                    (lambda (y)
                    (lambda (z)
                        (* z
                            (+ x y)))))
                2)
                3)
                4)",
            Some(expected),
            None,
            Some(terminal),
            None,
            23,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_comparison<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_true(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((x 2)
                    (y 3)
                    (z 4))
                (= 20 (* z
                        (+ x y))))",
            Some(expected),
            None,
            Some(terminal),
            None,
            21,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_conditional<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 5);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    (false (lambda (a)
                            (lambda (b)
                                b)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) true))",
            Some(expected),
            None,
            Some(terminal),
            None,
            35,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_conditional2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 6);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    (false (lambda (a)
                            (lambda (b)
                                b)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) false))",
            Some(expected),
            None,
            Some(terminal),
            None,
            32,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_fundamental_conditional_bug<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 5);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((true (lambda (a)
                            (lambda (b)
                                a)))
                    ;; NOTE: We cannot shadow IF because it is built-in.
                    (if- (lambda (a)
                            (lambda (c)
                            (lambda (cond)
                                ((cond a) c))))))
                (((if- 5) 6) true))",
            Some(expected),
            None,
            Some(terminal),
            None,
            32,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_fully_evaluates<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 10);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(if t (+ 5 5) 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_recursion<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base)
                                (lambda (exponent)
                                    (if (= 0 exponent)
                                        1
                                        (* base ((exp base) (- exponent 1))))))))
                        ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            66,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_recursion_multiarg<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp base (- exponent 1)))))))
                        (exp 5 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            69,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_recursion_optimized<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((exp (lambda (base)
                            (letrec ((base-inner
                                        (lambda (exponent)
                                            (if (= 0 exponent)
                                                1
                                                (* base (base-inner (- exponent 1)))))))
                                    base-inner))))
                ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            56,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_tail_recursion<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base)
                                (lambda (exponent-remaining)
                                    (lambda (acc)
                                    (if (= 0 exponent-remaining)
                                        acc
                                        (((exp base) (- exponent-remaining 1)) (* acc base))))))))
                        (((exp 5) 2) 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            93,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_tail_recursion_somewhat_optimized<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 25);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
        s_ref,        "(letrec ((exp (lambda (base)
                                (letrec ((base-inner
                                            (lambda (exponent-remaining)
                                            (lambda (acc)
                                                (if (= 0 exponent-remaining)
                                                    acc
                                                    ((base-inner (- exponent-remaining 1)) (* acc base)))))))
                                        base-inner))))
                        (((exp 5) 2) 1))",
        Some(expected),
        None,
        Some(terminal),
        None,
        81,None
    );
    }

    #[test]
    #[ignore]
    pub fn test_prove_no_mutual_recursion<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_true(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((even (lambda (n)
                                (if (= 0 n)
                                    t
                                    (odd (- n 1)))))
                        (odd (lambda (n)
                                (even (- n 1)))))
                    ;; NOTE: This is not true mutual-recursion.
                    ;; However, it exercises the behavior of LETREC.
                    (odd 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_no_mutual_recursion_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((even (lambda (n)
                                (if (= 0 n)
                                    t
                                    (odd (- n 1)))))
                        (odd (lambda (n)
                                (even (- n 1)))))
                    ;; NOTE: This is not true mutual-recursion.
                    ;; However, it exercises the behavior of LETREC.
                    (odd 2))",
            None,
            None,
            Some(error),
            None,
            25,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_cons1<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 1);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(car (cons 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_car_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(car (1 2) 3)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_cdr_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(cdr (1 2) 3)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_atom_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(atom 123 4)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_emit_end_is_nil_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(emit 123 4)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_cons2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 2);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(cdr (cons 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda1<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda () 123))",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 10);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((x 9) (f (lambda () (+ x 1)))) (f))",
            Some(expected),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda3<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = {
            let arg = s.intern_user_symbol("x");
            let num = G::ptr_for_num(&s, 123);
            let body = G::ptr_for_list(&s, &[num]);
            let env = G::ptr_for_nil(&s);
            G::ptr_for_fun(&s, arg, body, env)
        };
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);
        nova_test_full_aux::<_, _, G>(
            s_ref,
            "((lambda (x) 123))",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            DEFAULT_REDUCTION_COUNT,
            false,
            None,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda4<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda () 123) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda5<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = s.read("(123)").unwrap();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(123)",
            Some(expected),
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_zero_arg_lambda6<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 123);
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((emit 123))",
            Some(expected),
            None,
            Some(error),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_nested_let_closure_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let terminal = s.get_cont_terminal();
        let expected = G::ptr_for_num(&s, 6);
        let expr = "(let ((data-function (lambda () 123))
                        (x 6)
                        (data (data-function)))
                    x)";
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            14,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_minimal_tail_call<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec
                ((f (lambda (x)
                        (if (= x 3)
                            123
                            (f (+ x 1))))))
                (f 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            50,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_cons_in_function1<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 2);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(((lambda (a)
                (lambda (b)
                    (car (cons a b))))
                2)
                3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            15,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_cons_in_function2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 3);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(((lambda (a)
                (lambda (b)
                    (cdr (cons a b))))
                2)
                3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            15,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_multiarg_eval_bug<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 2);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(car (cdr '(1 2 3 4)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_multiple_letrec_bindings<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec
                ((x 888)
                (f (lambda (x)
                        (if (= x 5)
                            123
                            (f (+ x 1))))))
                (f 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            78,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_tail_call2<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec
                ((f (lambda (x)
                        (if (= x 5)
                            123
                            (f (+ x 1)))))
                (g (lambda (x) (f x))))
                (g 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            84,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_multiple_letrecstar_bindings<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 13);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((double (lambda (x) (* 2 x)))
                        (square (lambda (x) (* x x))))
                        (+ (square 3) (double 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_multiple_letrecstar_bindings_referencing<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 11);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((double (lambda (x) (* 2 x)))
                        (double-inc (lambda (x) (+ 1 (double x)))))
                        (+ (double 3) (double-inc 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            31,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_multiple_letrecstar_bindings_recursive<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 33);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((exp (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp base (- exponent 1))))))
                        (exp2 (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp2 base (- exponent 1))))))
                        (exp3 (lambda (base exponent)
                                (if (= 0 exponent)
                                    1
                                    (* base (exp3 base (- exponent 1)))))))
                        (+ (+ (exp 3 2) (exp2 2 3))
                        (exp3 4 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            242,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_dont_discard_rest_env<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 18);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((z 9))
                (letrec ((a 1)
                            (b 2)
                            (l (lambda (x) (+ z x))))
                        (l 9)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_fibonacci<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 1);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        nova_test_full_aux::<_, _, G>(
            s_ref,
            "(letrec ((next (lambda (a b n target)
                    (if (eq n target)
                        a
                        (next b
                            (+ a b)
                            (+ 1 n)
                        target))))
                (fib (next 0 1 0)))
            (fib 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            89,
            5,
            false,
            None,
            None,
        );
    }

    // TODO: finish porting to a generic expression
    // #[test]
    // #[ignore]
    // fn test_prove_fibonacci_100<
    //        'a,
    //        F: CurveCycleEquipped,
    //        C: Coprocessor<F> + 'a,
    //        G: MultiFrameExt<'a, F, C>,
    //    >()
    //    where
    //        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    //       <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    // {
    //     let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
    //     let expected = s.read("354224848179261915075");
    //     let terminal = s.get_cont_terminal();
    //     nova_test_full_aux::<_, _, G>(
    //         s,
    //         "(letrec ((next (lambda (a b n target)
    //                  (if (eq n target)
    //                      a
    //                      (next b
    //                          (+ a b)
    //                          (+ 1 n)
    //                         target))))
    //                 (fib (next 0 1 0)))
    //             (fib 100))",
    //         Some(expected),
    //         None,
    //         Some(terminal),
    //         None,
    //         4841,
    //         5,
    //         false,
    //     );
    // }

    #[test]
    pub fn test_prove_terminal_continuation_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((a (lambda (x) (cons 2 2))))
            (a 1))",
            None,
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_chained_functional_commitment<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((secret 12345)
                    (a (lambda (acc x)
                        (let ((acc (+ acc x)))
                            (cons acc (cons secret (a acc)))))))
            (a 0 5))",
            None,
            None,
            Some(terminal),
            None,
            39,
            None,
        );
    }

    #[test]
    pub fn test_prove_begin_empty<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(begin)",
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_begin_emit<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(begin (emit 1) (emit 2) (emit 3))";
        let expected_expr = G::ptr_for_num(&s, 3);
        let expected_emitted = vec![
            G::ptr_for_num(&s, 1),
            G::ptr_for_num(&s, 2),
            G::ptr_for_num(&s, 3),
        ];
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected_expr),
            None,
            None,
            Some(&expected_emitted),
            13,
            None,
        );
    }

    #[test]
    pub fn test_prove_str_car<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected_a = s.read(r"#\a").unwrap();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car "apple")"#,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_str_cdr<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected_pple = s.read(r#" "pple" "#).unwrap();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr "apple")"#,
            Some(expected_pple),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_str_car_empty<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected_nil = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car "")"#,
            Some(expected_nil),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_str_cdr_empty<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected_empty_str = s.intern_string("");
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr "")"#,
            Some(expected_empty_str),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_strcons<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected_apple = s.read(r#" "apple" "#).unwrap();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(strcons #\a "pple")"#,
            Some(expected_apple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_str_cons_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r"(strcons #\a 123)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_one_arg_cons_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(cons "")"#,
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    pub fn test_prove_car_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car nil)"#,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_cdr_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr nil)"#,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_car_cdr_invalid_tag_error_sym<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car car)"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr car)"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_car_cdr_invalid_tag_error_char<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, r"(car #\a)", None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, r"(cdr #\a)", None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_car_cdr_invalid_tag_error_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, r#"(car 42)"#, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, r#"(cdr 42)"#, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_car_cdr_of_cons<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let res1 = G::ptr_for_num(&s, 1);
        let res2 = G::ptr_for_num(&s, 2);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car (cons 1 2))"#,
            Some(res1),
            None,
            Some(terminal),
            None,
            5,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr (cons 1 2))"#,
            Some(res2),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_car_cdr_invalid_tag_error_lambda<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car (lambda (x) x))"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr (lambda (x) x))"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_hide_open<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (hide 123 456))";
        let expected = G::ptr_for_num(&s, 456);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_hide_wrong_secret_type<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(hide 'x 456)";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_hide_secret<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret (hide 123 456))";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_hide_open_sym<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (hide 123 'x))";
        let x = s.intern_user_symbol("x");
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(x), None, Some(terminal), None, 5, None);
    }

    #[test]
    pub fn test_prove_commit_open_sym<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (commit 'x))";
        let x = s.intern_user_symbol("x");
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(x), None, Some(terminal), None, 4, None);
    }

    #[test]
    pub fn test_prove_commit_open<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (commit 123))";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    pub fn test_prove_commit_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(commit 123 456)";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 1, None);
    }

    #[test]
    pub fn test_prove_open_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open 123 456)";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 1, None);
    }

    #[test]
    pub fn test_prove_open_wrong_type<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open 'asdf)";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_secret_wrong_type<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret 'asdf)";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_commit_secret<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret (commit 123))";
        let expected = G::ptr_for_num(&s, 0);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    pub fn test_prove_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(num 123)";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_num_char<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = r"(num #\a)";
        let expected = G::ptr_for_num(&s, 97);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_char_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = r#"(char 97)"#;
        let expected_a = s.read(r"#\a").unwrap();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_char_coercion<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = r#"(char (- 0 4294967200))"#;
        let expr2 = r#"(char (- 0 4294967199))"#;
        let expected_a = s.read(r"#\a").unwrap();
        let expected_b = s.read(r"#\b").unwrap();
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            5,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(expected_b),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_commit_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(num (commit 123))";
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(terminal), None, 4, None);
    }

    #[test]
    pub fn test_prove_hide_open_comm_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (comm (num (hide 123 456))))";
        let expected = G::ptr_for_num(&s, 456);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    pub fn test_prove_hide_secret_comm_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret (comm (num (hide 123 456))))";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    pub fn test_prove_commit_open_comm_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (comm (num (commit 123))))";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            8,
            None,
        );
    }

    #[test]
    pub fn test_prove_commit_secret_comm_num<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret (comm (num (commit 123))))";
        let expected = G::ptr_for_num(&s, 0);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            8,
            None,
        );
    }

    #[test]
    pub fn test_prove_commit_num_open<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open (num (commit 123)))";
        let expected = G::ptr_for_num(&s, 123);
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }

    #[test]
    pub fn test_prove_num_invalid_tag<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(num (quote x))";
        let expr1 = "(num \"asdf\")";
        let expr2 = "(num '(1))";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr1, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_comm_invalid_tag<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(comm (quote x))";
        let expr1 = "(comm \"asdf\")";
        let expr2 = "(comm '(1))";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr1, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_char_invalid_tag<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(char (quote x))";
        let expr1 = "(char \"asdf\")";
        let expr2 = "(char '(1))";
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr1, None, None, Some(error), None, 2, None);

        test_aux::<_, _, G>(s_ref, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    pub fn test_prove_terminal_sym<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(quote x)";
        let x = s.intern_user_symbol("x");
        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(x), None, Some(terminal), None, 1, None);
    }

    #[test]
    #[should_panic]
    pub fn test_prove_open_opaque_commit<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(open 123)";
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, None, None, 2, None);
    }

    #[test]
    #[should_panic]
    pub fn test_prove_secret_invalid_tag<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret 123)";
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, None, None, 2, None);
    }

    #[test]
    #[should_panic]
    pub fn test_prove_secret_opaque_commit<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(secret (comm 123))";
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, None, None, 2, None);
    }

    #[test]
    pub fn test_str_car_cdr_cons<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let a = s.read(r"#\a").unwrap();
        let apple = s.read(r#" "apple" "#).unwrap();
        let a_pple = s.read(r#" (#\a . "pple") "#).unwrap();
        let pple = s.read(r#" "pple" "#).unwrap();
        let empty = s.intern_string("");
        let nil = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let error = s.get_cont_error();

        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            r#"(car "apple")"#,
            Some(a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr "apple")"#,
            Some(pple),
            None,
            Some(terminal),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(car "")"#,
            Some(nil),
            None,
            Some(terminal),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cdr "")"#,
            Some(empty),
            None,
            Some(terminal),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(cons #\a "pple")"#,
            Some(a_pple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(strcons #\a "pple")"#,
            Some(apple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r"(strcons #\a #\b)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(strcons "a" "b")"#,
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            r#"(strcons 1 2)"#,
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    fn relational_aux<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >(
        s: &'a <G as MultiFrameTrait<'a, F, C>>::Store,
        op: &str,
        a: &str,
        b: &str,
        res: bool,
    ) where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let expr = &format!("({op} {a} {b})");
        let expected = if res {
            G::ptr_for_true(s)
        } else {
            G::ptr_for_nil(s)
        };
        let terminal = s.get_cont_terminal();

        test_aux::<_, _, G>(s, expr, Some(expected), None, Some(terminal), None, 3, None);
    }

    #[ignore]
    #[test]
    pub fn test_prove_test_relational<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let lt = "<";
        let gt = ">";
        let lte = "<=";
        let gte = ">=";
        let zero = "0";
        let one = "1";
        let two = "2";
        let s_ref: &'a _ = WAREHOUSE.store(s);

        let most_negative = &format!("{}", Num::<F>::most_negative());
        let most_positive = &format!("{}", Num::<F>::most_positive());
        let neg_one = &format!("{}", Num::<F>::Scalar(F::ZERO - F::ONE));

        relational_aux::<F, C, G>(s_ref, lt, one, two, true);
        relational_aux::<F, C, G>(s_ref, gt, one, two, false);
        relational_aux::<F, C, G>(s_ref, lte, one, two, true);
        relational_aux::<F, C, G>(s_ref, gte, one, two, false);

        relational_aux::<F, C, G>(s_ref, lt, two, one, false);
        relational_aux::<F, C, G>(s_ref, gt, two, one, true);
        relational_aux::<F, C, G>(s_ref, lte, two, one, false);
        relational_aux::<F, C, G>(s_ref, gte, two, one, true);

        relational_aux::<F, C, G>(s_ref, lt, one, one, false);
        relational_aux::<F, C, G>(s_ref, gt, one, one, false);
        relational_aux::<F, C, G>(s_ref, lte, one, one, true);
        relational_aux::<F, C, G>(s_ref, gte, one, one, true);

        relational_aux::<F, C, G>(s_ref, lt, zero, two, true);
        relational_aux::<F, C, G>(s_ref, gt, zero, two, false);
        relational_aux::<F, C, G>(s_ref, lte, zero, two, true);
        relational_aux::<F, C, G>(s_ref, gte, zero, two, false);

        relational_aux::<F, C, G>(s_ref, lt, two, zero, false);
        relational_aux::<F, C, G>(s_ref, gt, two, zero, true);
        relational_aux::<F, C, G>(s_ref, lte, two, zero, false);
        relational_aux::<F, C, G>(s_ref, gte, two, zero, true);

        relational_aux::<F, C, G>(s_ref, lt, zero, zero, false);
        relational_aux::<F, C, G>(s_ref, gt, zero, zero, false);
        relational_aux::<F, C, G>(s_ref, lte, zero, zero, true);
        relational_aux::<F, C, G>(s_ref, gte, zero, zero, true);

        relational_aux::<F, C, G>(s_ref, lt, most_negative, zero, true);
        relational_aux::<F, C, G>(s_ref, gt, most_negative, zero, false);
        relational_aux::<F, C, G>(s_ref, lte, most_negative, zero, true);
        relational_aux::<F, C, G>(s_ref, gte, most_negative, zero, false);

        relational_aux::<F, C, G>(s_ref, lt, zero, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gt, zero, most_negative, true);
        relational_aux::<F, C, G>(s_ref, lte, zero, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gte, zero, most_negative, true);

        relational_aux::<F, C, G>(s_ref, lt, most_negative, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gt, most_negative, most_positive, false);
        relational_aux::<F, C, G>(s_ref, lte, most_negative, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gte, most_negative, most_positive, false);

        relational_aux::<F, C, G>(s_ref, lt, most_positive, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gt, most_positive, most_negative, true);
        relational_aux::<F, C, G>(s_ref, lte, most_positive, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gte, most_positive, most_negative, true);

        relational_aux::<F, C, G>(s_ref, lt, most_negative, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gt, most_negative, most_negative, false);
        relational_aux::<F, C, G>(s_ref, lte, most_negative, most_negative, true);
        relational_aux::<F, C, G>(s_ref, gte, most_negative, most_negative, true);

        relational_aux::<F, C, G>(s_ref, lt, one, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gt, one, most_positive, false);
        relational_aux::<F, C, G>(s_ref, lte, one, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gte, one, most_positive, false);

        relational_aux::<F, C, G>(s_ref, lt, most_positive, one, false);
        relational_aux::<F, C, G>(s_ref, gt, most_positive, one, true);
        relational_aux::<F, C, G>(s_ref, lte, most_positive, one, false);
        relational_aux::<F, C, G>(s_ref, gte, most_positive, one, true);

        relational_aux::<F, C, G>(s_ref, lt, one, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gt, one, most_negative, true);
        relational_aux::<F, C, G>(s_ref, lte, one, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gte, one, most_negative, true);

        relational_aux::<F, C, G>(s_ref, lt, most_negative, one, true);
        relational_aux::<F, C, G>(s_ref, gt, most_negative, one, false);
        relational_aux::<F, C, G>(s_ref, lte, most_negative, one, true);
        relational_aux::<F, C, G>(s_ref, gte, most_negative, one, false);

        relational_aux::<F, C, G>(s_ref, lt, neg_one, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gt, neg_one, most_positive, false);
        relational_aux::<F, C, G>(s_ref, lte, neg_one, most_positive, true);
        relational_aux::<F, C, G>(s_ref, gte, neg_one, most_positive, false);

        relational_aux::<F, C, G>(s_ref, lt, most_positive, neg_one, false);
        relational_aux::<F, C, G>(s_ref, gt, most_positive, neg_one, true);
        relational_aux::<F, C, G>(s_ref, lte, most_positive, neg_one, false);
        relational_aux::<F, C, G>(s_ref, gte, most_positive, neg_one, true);

        relational_aux::<F, C, G>(s_ref, lt, neg_one, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gt, neg_one, most_negative, true);
        relational_aux::<F, C, G>(s_ref, lte, neg_one, most_negative, false);
        relational_aux::<F, C, G>(s_ref, gte, neg_one, most_negative, true);

        relational_aux::<F, C, G>(s_ref, lt, most_negative, neg_one, true);
        relational_aux::<F, C, G>(s_ref, gt, most_negative, neg_one, false);
        relational_aux::<F, C, G>(s_ref, lte, most_negative, neg_one, true);
        relational_aux::<F, C, G>(s_ref, gte, most_negative, neg_one, false);
    }

    #[test]
    pub fn test_relational_edge_case_identity<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        // Normally, a value cannot be less than the result of incrementing it.
        // However, the most positive field element (when viewed as signed)
        // is the exception. Incrementing it yields the most negative element,
        // which is less than the most positive.
        let expr = "(let ((most-positive (/ (- 0 1) 2))
                        (most-negative (+ 1 most-positive)))
                    (< most-negative most-positive))";
        let t = G::ptr_for_true(&s);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(t), None, Some(terminal), None, 19, None);
    }

    #[test]
    pub fn test_prove_test_eval<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(* 3 (eval  (cons '+ (cons 1 (cons 2 nil)))))";
        let expr2 = "(* 5 (eval '(+ 1 a) '((a . 3))))"; // two-arg eval, optional second arg is env.
        let res = G::ptr_for_num(&s, 9);
        let res2 = G::ptr_for_num(&s, 20);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 17, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_keyword<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = ":asdf";
        let expr2 = "(eq :asdf :asdf)";
        let expr3 = "(eq :asdf 'asdf)";
        let res = s.key("asdf");
        let res2 = G::ptr_for_true(&s);
        let res3 = G::ptr_for_nil(&s);

        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 1, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            expr3,
            Some(res3),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    // The following functional commitment tests were discovered to fail. They are commented out (as tests) for now so
    // they can be addressed independently in future work.

    #[test]
    pub fn test_prove_functional_commitment<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(let ((f (commit (let ((num 9)) (lambda (f) (f num)))))
                        (inc (lambda (x) (+ x 1))))
                    ((open f) inc))";
        let res = G::ptr_for_num(&s, 10);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 25, None);
    }

    #[test]
    #[ignore]
    pub fn test_prove_complicated_functional_commitment<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(let ((f (commit (let ((nums '(1 2 3))) (lambda (f) (f nums)))))
                        (in (letrec ((sum-aux (lambda (acc nums)
                                            (if nums
                                            (sum-aux (+ acc (car nums)) (cdr nums))
                                            acc)))
                                (sum (sum-aux 0)))
                            (lambda (nums)
                            (sum nums)))))

                    ((open f) in))";
        let res = G::ptr_for_num(&s, 6);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(res),
            None,
            Some(terminal),
            None,
            108,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_fold_cons_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(letrec ((fold (lambda (op acc l)
                                    (if l
                                        (fold op (op acc (car l)) (cdr l))
                                        acc))))
                    (fold (lambda (x y) (+ x y)) 0 '(1 2 3)))";
        let res = G::ptr_for_num(&s, 6);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            expr,
            Some(res),
            None,
            Some(terminal),
            None,
            152,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_lambda_args_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(cons (lambda (x y) nil) nil)";
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(terminal), None, 3, None);
    }

    #[test]
    pub fn test_prove_reduce_sym_contradiction_regression<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(eval 'a '(nil))";
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 4, None);
    }

    #[test]
    pub fn test_prove_test_self_eval_env_not_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        // NOTE: cond1 shouldn't depend on env-is-not-nil
        // therefore this unit test is not very useful
        // the conclusion is that by removing condition env-is-not-nil from cond1,
        // we solve this soundness problem
        // this solution makes the circuit a bit smaller
        let expr = "(let ((a 1)) t)";

        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(terminal), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_self_eval_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        // nil doesn't have SYM tag
        let expr = "nil";

        let terminal = s.get_cont_terminal();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(terminal), None, 1, None);
    }

    #[test]
    pub fn test_prove_test_env_not_nil_and_binding_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(let ((a 1) (b 2)) c)";

        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 7, None);
    }

    #[test]
    pub fn test_prove_test_eval_bad_form<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(* 5 (eval '(+ 1 a) '((0 . 3))))"; // two-arg eval, optional second arg is env. This tests for error on malformed env.
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 8, None);
    }

    #[test]
    pub fn test_prove_test_u64_self_evaluating<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "123u64";
        let res = G::ptr_for_u64(&s, 123);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 1, None);
    }

    #[test]
    pub fn test_prove_test_u64_mul<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(* (u64 18446744073709551615) (u64 2))";
        let expr2 = "(* 18446744073709551615u64 2u64)";
        let expr3 = "(* (- 0u64 1u64) 2u64)";
        let expr4 = "(u64 18446744073709551617)";
        let res = G::ptr_for_u64(&s, 18446744073709551614);
        let res2 = G::ptr_for_u64(&s, 1);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 7, None);

        test_aux::<_, _, G>(s_ref, expr2, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr3, Some(res), None, Some(terminal), None, 6, None);

        test_aux::<_, _, G>(
            s_ref,
            expr4,
            Some(res2),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_u64_add<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(+ 18446744073709551615u64 2u64)";
        let expr2 = "(+ (- 0u64 1u64) 2u64)";
        let res = G::ptr_for_u64(&s, 1);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr2, Some(res), None, Some(terminal), None, 6, None);
    }

    #[test]
    pub fn test_prove_test_u64_sub<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(- 2u64 1u64)";
        let expr2 = "(- 0u64 1u64)";
        let expr3 = "(+ 1u64 (- 0u64 1u64))";
        let res = G::ptr_for_u64(&s, 1);
        let res2 = G::ptr_for_u64(&s, 18446744073709551615);
        let res3 = G::ptr_for_u64(&s, 0);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            expr3,
            Some(res3),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_u64_div<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(/ 100u64 2u64)";
        let res = G::ptr_for_u64(&s, 50);

        let expr2 = "(/ 100u64 3u64)";
        let res2 = G::ptr_for_u64(&s, 33);

        let expr3 = "(/ 100u64 0u64)";

        let terminal = s.get_cont_terminal();
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(s_ref, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_u64_mod<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(% 100u64 2u64)";
        let res = G::ptr_for_u64(&s, 0);

        let expr2 = "(% 100u64 3u64)";
        let res2 = G::ptr_for_u64(&s, 1);

        let expr3 = "(% 100u64 0u64)";

        let terminal = s.get_cont_terminal();
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(s_ref, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_num_mod<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(% 100 3)";
        let expr2 = "(% 100 3u64)";
        let expr3 = "(% 100u64 3)";

        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr2, None, None, Some(error), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_u64_comp<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(< 0u64 1u64)";
        let expr2 = "(< 1u64 0u64)";
        let expr3 = "(<= 0u64 1u64)";
        let expr4 = "(<= 1u64 0u64)";

        let expr5 = "(> 0u64 1u64)";
        let expr6 = "(> 1u64 0u64)";
        let expr7 = "(>= 0u64 1u64)";
        let expr8 = "(>= 1u64 0u64)";

        let expr9 = "(<= 0u64 0u64)";
        let expr10 = "(>= 0u64 0u64)";

        let t = G::ptr_for_true(&s);
        let nil = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr2, Some(nil), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr3, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr4, Some(nil), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr5, Some(nil), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr6, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr7, Some(nil), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr8, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr9, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr10, Some(t), None, Some(terminal), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_u64_conversion<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(+ 0 1u64)";
        let expr2 = "(num 1u64)";
        let expr3 = "(+ 1 1u64)";
        let expr4 = "(u64 (+ 1 1))";
        let res = G::ptr_for_num(&s, 1);
        let res2 = G::ptr_for_num(&s, 2);
        let res3 = G::ptr_for_u64(&s, 2);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr2, Some(res), None, Some(terminal), None, 2, None);

        test_aux::<_, _, G>(
            s_ref,
            expr3,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            expr4,
            Some(res3),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_u64_num_comparison<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(= 1 1u64)";
        let expr2 = "(= 1 2u64)";
        let t = G::ptr_for_true(&s);
        let nil = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(s_ref, expr2, Some(nil), None, Some(terminal), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_u64_num_cons<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(cons 1 1u64)";
        let expr2 = "(cons 1u64 1)";
        let res = s.read("(1 . 1u64)").unwrap();
        let res2 = s.read("(1u64 . 1)").unwrap();
        let terminal = s.get_cont_terminal();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, Some(res), None, Some(terminal), None, 3, None);

        test_aux::<_, _, G>(
            s_ref,
            expr2,
            Some(res2),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    pub fn test_prove_test_hide_u64_secret<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(hide 0u64 123)";
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_test_mod_by_zero_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();

        let expr = "(% 0 0)";
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_dotted_syntax_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expr = "(let ((a (lambda (x) (+ x 1)))) (a . 1))";
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(s_ref, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    pub fn test_prove_call_literal_fun<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let empty_env = G::ptr_for_nil(&s);
        let arg = s.intern_user_symbol("x");
        let body = s.read("((+ x 1))").unwrap();
        let fun = G::ptr_for_fun(&s, arg, body, empty_env);
        let input = G::ptr_for_num(&s, 9);
        let expr = G::ptr_for_list(&s, &[fun, input]);
        let res = G::ptr_for_num(&s, 10);
        let terminal = s.get_cont_terminal();
        let lang: std::sync::Arc<crate::proof::Lang<F, C>> =
            std::sync::Arc::new(crate::proof::Lang::new());
        let s_ref: &'a _ = WAREHOUSE.store(s);

        nova_test_full_aux2::<_, _, G>(
            s_ref,
            expr,
            Some(res),
            None,
            Some(terminal),
            None,
            7,
            DEFAULT_REDUCTION_COUNT,
            false,
            None,
            lang,
        );
    }

    #[test]
    pub fn test_prove_lambda_body_syntax<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();

        let s_ref: &'a _ = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda ()))",
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "((lambda () 1 2))",
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (x)) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (x) 1 2) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    pub fn test_prove_non_symbol_binding_error<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let error = s.get_cont_error();
        let s_ref: &'a _ = WAREHOUSE.store(s);

        let test = |x| {
            let expr = format!("(let (({x} 123)) {x})");
            let expr2 = format!("(letrec (({x} 123)) {x})");
            let expr3 = format!("(lambda ({x}) {x})");

            test_aux::<_, _, G>(s_ref, &expr, None, None, Some(error), None, 1, None);

            test_aux::<_, _, G>(s_ref, &expr2, None, None, Some(error), None, 1, None);

            test_aux::<_, _, G>(s_ref, &expr3, None, None, Some(error), None, 1, None);
        };

        test(":a");
        test("1");
        test("\"string\"");
        test("1u64");
        test("#\\x");
    }

    // This is related to issue #426
    #[test]
    pub fn test_prove_lambda_body_nil<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_nil(&s);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "((lambda (x) nil) 0)",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    // The following 3 tests are related to issue #424
    #[test]
    pub fn test_letrec_let_nesting<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 2);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((x (let ((z 0)) 1))) 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }
    #[test]
    pub fn test_let_sequencing<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 1);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(let ((x 0) (y x)) 1)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }
    #[test]
    pub fn test_letrec_sequencing<
        'a,
        F: CurveCycleEquipped,
        C: Coprocessor<F> + 'a,
        G: MultiFrameExt<'a, F, C>,
    >()
    where
        <<G1<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
        <<G2<F> as Group>::Scalar as ff::PrimeField>::Repr: Abomonation,
    {
        let s = <G as MultiFrameTrait<'a, F, C>>::Store::default();
        let expected = G::ptr_for_num(&s, 3);
        let terminal = s.get_cont_terminal();
        let s_ref = WAREHOUSE.store(s);

        test_aux::<_, _, G>(
            s_ref,
            "(letrec ((x 0) (y (letrec ((inner 1)) 2))) 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            8,
            None,
        );
    }
}

////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod fancy_lurk_tests {
    use super::proof_tests;
    use crate::proof::nova_tests::{C1Lurk, Coproc};
    use pasta_curves::pallas::Scalar as Fr;

    instantiate_proof_tests!(Fr, Coproc<Fr>, C1Lurk<'_, Fr, Coproc<Fr>>);
}

#[cfg(test)]
mod fancy_lem_tests {
    use super::proof_tests;
    use crate::proof::nova_tests::{Coproc, C1LEM};
    use pasta_curves::pallas::Scalar as Fr;

    instantiate_proof_tests!(Fr, Coproc<Fr>, C1LEM<'_, Fr, Coproc<Fr>>);
}
////////////////////////////////////////////////////////////////////////////////

pub mod tests_lem {
    use pasta_curves::pallas::Scalar as Fr;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use super::{nova_test_full_aux, nova_test_full_aux2, test_aux, DEFAULT_REDUCTION_COUNT};

    use crate::eval::lang::Coproc;
    use crate::eval::lang::Lang;
    use crate::num::Num;
    use crate::proof::nova::*;
    use crate::state::user_sym;
    use crate::state::State;
    use crate::tag::{
        ContTag::{Error, Terminal},
        ExprTag, Op, Op1, Op2,
    };

    use crate::lem::pointers::Ptr;
    use crate::lem::store::Store;
    use crate::lem::Tag;

    type M1<'a, Fr> = C1LEM<'a, Fr, Coproc<Fr>>;

    #[test]
    fn test_prove_binop() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(+ 1 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[should_panic]
    // This tests the testing mechanism. Since the supplied expected value is wrong,
    // the test should panic on an assertion failure.
    fn test_prove_binop_fail() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(+ 1 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_arithmetic_let() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((a 5)
                      (b 1)
                      (c 2))
                 (/ (+ a b) c))",
            Some(expected),
            None,
            Some(terminal),
            None,
            18,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_eq() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(eq 5 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            DEFAULT_REDUCTION_COUNT,
            true,
            None,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_num_equal() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(= 5 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(= 5 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_invalid_num_equal() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(= 5 nil)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );

        let expected = Ptr::num_u64(5);
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(= nil 5)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_equal() {
        let s = &Store::<Fr>::default();
        let nil = s.intern_nil();
        let t = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(
            s,
            "(eq 5 nil)",
            Some(nil),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(eq nil 5)",
            Some(nil),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(eq nil nil)",
            Some(t),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(s, "(eq 5 5)", Some(t), None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_quote_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(quote (1) (2))", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_if() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(5);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(if t 5 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        let expected = Ptr::num_u64(6);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(if nil 5 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        )
    }

    #[test]
    fn test_prove_if_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(5);
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(if nil 5 6 7)",
            Some(expected),
            None,
            Some(error),
            None,
            2,
            None,
        )
    }

    #[test]
    #[ignore]
    fn test_prove_if_fully_evaluates() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(10);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(if t (+ 5 5) 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    #[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
    fn test_prove_recursion1() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base)
                               (lambda (exponent)
                                 (if (= 0 exponent)
                                     1
                                     (* base ((exp base) (- exponent 1))))))))
                 ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            66,
            None,
        );
    }

    #[test]
    #[ignore] // Skip expensive tests in CI for now. Do run these locally, please.
    fn test_prove_recursion2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base)
                                  (lambda (exponent)
                                     (lambda (acc)
                                       (if (= 0 exponent)
                                          acc
                                          (((exp base) (- exponent 1)) (* acc base))))))))
                (((exp 5) 2) 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            93,
            None,
        );
    }

    fn test_prove_unop_regression_aux(chunk_count: usize) {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(atom 123)",
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            chunk_count, // This needs to be 1 to exercise the bug.
            false,
            None,
            None,
        );

        let expected = Ptr::num_u64(1);
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(car '(1 . 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            chunk_count, // This needs to be 1 to exercise the bug.
            false,
            None,
            None,
        );

        let expected = Ptr::num_u64(2);
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(cdr '(1 . 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            chunk_count, // This needs to be 1 to exercise the bug.
            false,
            None,
            None,
        );

        let expected = Ptr::num_u64(123);
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(emit 123)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            chunk_count,
            false,
            None,
            None,
        )
    }

    #[test]
    #[ignore]
    fn test_prove_unop_regression() {
        // We need to at least use chunk size 1 to exercise the regression.
        // Also use a non-1 value to check the MultiFrame case.
        for i in 1..2 {
            test_prove_unop_regression_aux(i);
        }
    }

    #[test]
    #[ignore]
    fn test_prove_emit_output() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(emit 123)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(99);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x) x) 99)",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(99);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (y)
                    ((lambda (x) y) 888))
                  99)",
            Some(expected),
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate3() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(999);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (y)
                     ((lambda (x)
                        ((lambda (z) z)
                         x))
                      y))
                   999)",
            Some(expected),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate4() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(888);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (y)
                     ((lambda (x)
                        ((lambda (z) z)
                         x))
                      ;; NOTE: We pass a different value here.
                      888))
                  999)",
            Some(expected),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate5() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(999);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(((lambda (fn)
                      (lambda (x) (fn x)))
                    (lambda (y) y))
                   999)",
            Some(expected),
            None,
            Some(terminal),
            None,
            13,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_evaluate_sum() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(9);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(+ 2 (+ 3 4))",
            Some(expected),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }

    #[test]
    fn test_prove_binop_rest_is_nil() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(9);
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(- 9 8 7)",
            Some(expected),
            None,
            Some(error),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(= 9 8 7)",
            Some(expected),
            None,
            Some(error),
            None,
            2,
            None,
        );
    }

    fn op_syntax_error<T: Op + Copy>() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        let test = |op: T| {
            let name = op.symbol_name();

            if !op.supports_arity(0) {
                let expr = format!("({name})");
                tracing::debug!("{:?}", &expr);
                test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
            }
            if !op.supports_arity(1) {
                let expr = format!("({name} 123)");
                tracing::debug!("{:?}", &expr);
                test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
            }
            if !op.supports_arity(2) {
                let expr = format!("({name} 123 456)");
                tracing::debug!("{:?}", &expr);
                test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
            }

            if !op.supports_arity(3) {
                let expr = format!("({name} 123 456 789)");
                tracing::debug!("{:?}", &expr);
                let iterations = if op.supports_arity(2) { 2 } else { 1 };
                test_aux::<_, _, M1<'_, _>>(
                    s,
                    &expr,
                    None,
                    None,
                    Some(error),
                    None,
                    iterations,
                    None,
                );
            }
        };

        for op in T::all() {
            test(*op);
        }
    }

    #[test]
    #[ignore]
    fn test_prove_unop_syntax_error() {
        op_syntax_error::<Op1>();
    }

    #[test]
    #[ignore]
    fn test_prove_binop_syntax_error() {
        op_syntax_error::<Op2>();
    }

    #[test]
    fn test_prove_diff() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(4);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(- 9 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_product() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(45);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(* 9 5)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_quotient() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(7);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(/ 21 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_error_div_by_zero() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(0);
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(/ 21 0)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_error_invalid_type_and_not_cons() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(/ 21 nil)",
            Some(expected),
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_adder() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(5);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(((lambda (x)
                    (lambda (y)
                      (+ x y)))
                  2)
                 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            13,
            None,
        );
    }

    #[test]
    fn test_prove_current_env_simple() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(current-env)",
            Some(expected),
            None,
            Some(terminal),
            None,
            1,
            None,
        );
    }

    #[test]
    fn test_prove_current_env_rest_is_nil_error() {
        let s = &Store::<Fr>::default();
        let expected = s.read_with_default_state("(current-env a)").unwrap();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(current-env a)",
            Some(expected),
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_let_simple() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((a 1))
                  a)",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_let_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((a 1 2)) a)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    fn test_prove_letrec_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((a 1 2)) a)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    fn test_prove_lambda_empty_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x)) 0)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_let_empty_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(let)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_let_empty_body_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(let ((a 1)))", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_letrec_empty_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(letrec)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_letrec_empty_body_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((a 1)))",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    fn test_prove_let_body_nil() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(eq nil (let () nil))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    fn test_prove_let_rest_body_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((a 1)) a 1)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    fn test_prove_letrec_rest_body_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((a 1)) a 1)",
            None,
            None,
            Some(error),
            None,
            1,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_let_null_bindings() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let () (+ 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }
    #[test]
    #[ignore]
    fn test_prove_letrec_null_bindings() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec () (+ 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_let() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(6);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((a 1)
                       (b 2)
                       (c 3))
                  (+ a (+ b c)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            18,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_arithmetic() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(20);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((((lambda (x)
                      (lambda (y)
                        (lambda (z)
                          (* z
                             (+ x y)))))
                    2)
                  3)
                 4)",
            Some(expected),
            None,
            Some(terminal),
            None,
            23,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_comparison() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((x 2)
                       (y 3)
                       (z 4))
                  (= 20 (* z
                           (+ x y))))",
            Some(expected),
            None,
            Some(terminal),
            None,
            21,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_conditional() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(5);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((true (lambda (a)
                               (lambda (b)
                                 a)))
                       (false (lambda (a)
                                (lambda (b)
                                  b)))
                      ;; NOTE: We cannot shadow IF because it is built-in.
                      (if- (lambda (a)
                             (lambda (c)
                               (lambda (cond)
                                 ((cond a) c))))))
                 (((if- 5) 6) true))",
            Some(expected),
            None,
            Some(terminal),
            None,
            35,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_conditional2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(6);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((true (lambda (a)
                               (lambda (b)
                                 a)))
                       (false (lambda (a)
                                (lambda (b)
                                  b)))
                      ;; NOTE: We cannot shadow IF because it is built-in.
                      (if- (lambda (a)
                             (lambda (c)
                               (lambda (cond)
                                 ((cond a) c))))))
                 (((if- 5) 6) false))",
            Some(expected),
            None,
            Some(terminal),
            None,
            32,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_fundamental_conditional_bug() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(5);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((true (lambda (a)
                               (lambda (b)
                                 a)))
                       ;; NOTE: We cannot shadow IF because it is built-in.
                       (if- (lambda (a)
                              (lambda (c)
                               (lambda (cond)
                                 ((cond a) c))))))
                 (((if- 5) 6) true))",
            Some(expected),
            None,
            Some(terminal),
            None,
            32,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_fully_evaluates() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(10);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(if t (+ 5 5) 6)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_recursion() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base)
                                   (lambda (exponent)
                                     (if (= 0 exponent)
                                         1
                                         (* base ((exp base) (- exponent 1))))))))
                           ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            66,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_recursion_multiarg() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base exponent)
                                   (if (= 0 exponent)
                                       1
                                       (* base (exp base (- exponent 1)))))))
                           (exp 5 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            69,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_recursion_optimized() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((exp (lambda (base)
                                (letrec ((base-inner
                                           (lambda (exponent)
                                             (if (= 0 exponent)
                                                 1
                                                 (* base (base-inner (- exponent 1)))))))
                                        base-inner))))
                   ((exp 5) 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            56,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_tail_recursion() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base)
                                   (lambda (exponent-remaining)
                                     (lambda (acc)
                                       (if (= 0 exponent-remaining)
                                           acc
                                           (((exp base) (- exponent-remaining 1)) (* acc base))))))))
                          (((exp 5) 2) 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            93,None
        );
    }

    #[test]
    #[ignore]
    fn test_prove_tail_recursion_somewhat_optimized() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(25);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base)
                                   (letrec ((base-inner
                                              (lambda (exponent-remaining)
                                                (lambda (acc)
                                                  (if (= 0 exponent-remaining)
                                                      acc
                                                     ((base-inner (- exponent-remaining 1)) (* acc base)))))))
                                           base-inner))))
                          (((exp 5) 2) 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            81,None
        );
    }

    #[test]
    #[ignore]
    fn test_prove_no_mutual_recursion() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((even (lambda (n)
                                  (if (= 0 n)
                                      t
                                      (odd (- n 1)))))
                          (odd (lambda (n)
                                 (even (- n 1)))))
                        ;; NOTE: This is not true mutual-recursion.
                        ;; However, it exercises the behavior of LETREC.
                        (odd 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_no_mutual_recursion_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((even (lambda (n)
                                  (if (= 0 n)
                                      t
                                      (odd (- n 1)))))
                          (odd (lambda (n)
                                 (even (- n 1)))))
                        ;; NOTE: This is not true mutual-recursion.
                        ;; However, it exercises the behavior of LETREC.
                        (odd 2))",
            None,
            None,
            Some(error),
            None,
            25,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_cons1() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(car (cons 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    fn test_prove_car_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(car (1 2) 3)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_cdr_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(cdr (1 2) 3)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_atom_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(atom 123 4)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_emit_end_is_nil_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(emit 123 4)", None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_cons2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(cdr (cons 1 2))",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    fn test_prove_zero_arg_lambda1() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda () 123))",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_zero_arg_lambda2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(10);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((x 9) (f (lambda () (+ x 1)))) (f))",
            Some(expected),
            None,
            Some(terminal),
            None,
            10,
            None,
        );
    }

    #[test]
    fn test_prove_zero_arg_lambda3() {
        let s = &Store::<Fr>::default();
        let expected = {
            let arg = s.intern_user_symbol("x");
            let num = Ptr::num_u64(123);
            let body = s.list(vec![num]);
            let env = s.intern_nil();
            s.intern_3_ptrs(Tag::Expr(ExprTag::Fun), arg, body, env)
        };
        let terminal = Ptr::null(Tag::Cont(Terminal));
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x) 123))",
            Some(expected),
            None,
            Some(terminal),
            None,
            3,
            DEFAULT_REDUCTION_COUNT,
            false,
            None,
            None,
        );
    }

    #[test]
    fn test_prove_zero_arg_lambda4() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda () 123) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_zero_arg_lambda5() {
        let s = &Store::<Fr>::default();
        let expected = s.read_with_default_state("(123)").unwrap();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, "(123)", Some(expected), None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_zero_arg_lambda6() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((emit 123))",
            Some(expected),
            None,
            Some(error),
            None,
            5,
            None,
        );
    }

    #[test]
    fn test_prove_nested_let_closure_regression() {
        let s = &Store::<Fr>::default();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        let expected = Ptr::num_u64(6);
        let expr = "(let ((data-function (lambda () 123))
                          (x 6)
                          (data (data-function)))
                      x)";
        test_aux::<_, _, M1<'_, _>>(
            s,
            expr,
            Some(expected),
            None,
            Some(terminal),
            None,
            14,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_minimal_tail_call() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec
                   ((f (lambda (x)
                         (if (= x 3)
                             123
                             (f (+ x 1))))))
                   (f 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            50,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_cons_in_function1() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(((lambda (a)
                    (lambda (b)
                      (car (cons a b))))
                  2)
                 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            15,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_cons_in_function2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(((lambda (a)
                    (lambda (b)
                      (cdr (cons a b))))
                  2)
                 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            15,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_multiarg_eval_bug() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(car (cdr '(1 2 3 4)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_multiple_letrec_bindings() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec
                   ((x 888)
                    (f (lambda (x)
                         (if (= x 5)
                             123
                             (f (+ x 1))))))
                  (f 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            78,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_tail_call2() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec
                   ((f (lambda (x)
                         (if (= x 5)
                             123
                             (f (+ x 1)))))
                    (g (lambda (x) (f x))))
                  (g 0))",
            Some(expected),
            None,
            Some(terminal),
            None,
            84,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_multiple_letrecstar_bindings() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(13);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((double (lambda (x) (* 2 x)))
                           (square (lambda (x) (* x x))))
                          (+ (square 3) (double 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_multiple_letrecstar_bindings_referencing() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(11);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((double (lambda (x) (* 2 x)))
                           (double-inc (lambda (x) (+ 1 (double x)))))
                          (+ (double 3) (double-inc 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            31,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_multiple_letrecstar_bindings_recursive() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(33);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((exp (lambda (base exponent)
                                  (if (= 0 exponent)
                                      1
                                      (* base (exp base (- exponent 1))))))
                           (exp2 (lambda (base exponent)
                                   (if (= 0 exponent)
                                      1
                                      (* base (exp2 base (- exponent 1))))))
                          (exp3 (lambda (base exponent)
                                  (if (= 0 exponent)
                                      1
                                      (* base (exp3 base (- exponent 1)))))))
                         (+ (+ (exp 3 2) (exp2 2 3))
                            (exp3 4 2)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            242,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_dont_discard_rest_env() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(18);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((z 9))
                   (letrec ((a 1)
                             (b 2)
                             (l (lambda (x) (+ z x))))
                            (l 9)))",
            Some(expected),
            None,
            Some(terminal),
            None,
            22,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_fibonacci() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        nova_test_full_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((next (lambda (a b n target)
                     (if (eq n target)
                         a
                         (next b
                             (+ a b)
                             (+ 1 n)
                            target))))
                    (fib (next 0 1 0)))
                (fib 1))",
            Some(expected),
            None,
            Some(terminal),
            None,
            89,
            5,
            false,
            None,
            None,
        );
    }

    // #[test]
    // #[ignore]
    // fn test_prove_fibonacci_100() {
    //     let s = &Store::<Fr>::default();
    //     let expected = s.read_with_default_state("354224848179261915075").unwrap();
    //     let terminal = Ptr::null(Tag::Cont(Terminal));
    //     nova_test_full_aux::<Coproc<Fr>>::(
    //         s,
    //         "(letrec ((next (lambda (a b n target)
    //                  (if (eq n target)
    //                      a
    //                      (next b
    //                          (+ a b)
    //                          (+ 1 n)
    //                         target))))
    //                 (fib (next 0 1 0)))
    //             (fib 100))",
    //         Some(expected),
    //         None,
    //         Some(terminal),
    //         None,
    //         4841,
    //         5,
    //         false,
    //     );
    // }

    #[test]
    fn test_prove_terminal_continuation_regression() {
        let s = &Store::<Fr>::default();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((a (lambda (x) (cons 2 2))))
               (a 1))",
            None,
            None,
            Some(terminal),
            None,
            9,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_chained_functional_commitment() {
        let s = &Store::<Fr>::default();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((secret 12345)
                      (a (lambda (acc x)
                           (let ((acc (+ acc x)))
                             (cons acc (cons secret (a acc)))))))
                (a 0 5))",
            None,
            None,
            Some(terminal),
            None,
            39,
            None,
        );
    }

    #[test]
    fn test_prove_begin_empty() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(begin)",
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_begin_emit() {
        let s = &Store::<Fr>::default();
        let expr = "(begin (emit 1) (emit 2) (emit 3))";
        let expected_expr = Ptr::num_u64(3);
        let expected_emitted = vec![Ptr::num_u64(1), Ptr::num_u64(2), Ptr::num_u64(3)];
        test_aux::<_, _, M1<'_, _>>(
            s,
            expr,
            Some(expected_expr),
            None,
            None,
            Some(&expected_emitted),
            13,
            None,
        );
    }

    #[test]
    fn test_prove_str_car() {
        let s = &Store::<Fr>::default();
        let expected_a = s.read_with_default_state(r"#\a").unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car "apple")"#,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_str_cdr() {
        let s = &Store::<Fr>::default();
        let expected_pple = s.read_with_default_state(r#" "pple" "#).unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr "apple")"#,
            Some(expected_pple),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_str_car_empty() {
        let s = &Store::<Fr>::default();
        let expected_nil = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car "")"#,
            Some(expected_nil),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_str_cdr_empty() {
        let s = &Store::<Fr>::default();
        let expected_empty_str = s.intern_string("");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr "")"#,
            Some(expected_empty_str),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_strcons() {
        let s = &Store::<Fr>::default();
        let expected_apple = s.read_with_default_state(r#" "apple" "#).unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(strcons #\a "pple")"#,
            Some(expected_apple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_str_cons_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r"(strcons #\a 123)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    fn test_prove_one_arg_cons_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, r#"(cons "")"#, None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_car_nil() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car nil)"#,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_cdr_nil() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr nil)"#,
            Some(expected),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_car_cdr_invalid_tag_error_sym() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, r#"(car car)"#, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, r#"(cdr car)"#, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_car_cdr_invalid_tag_error_char() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, r"(car #\a)", None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, r"(cdr #\a)", None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_car_cdr_invalid_tag_error_num() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, r#"(car 42)"#, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, r#"(cdr 42)"#, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_car_cdr_of_cons() {
        let s = &Store::<Fr>::default();
        let res1 = Ptr::num_u64(1);
        let res2 = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car (cons 1 2))"#,
            Some(res1),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr (cons 1 2))"#,
            Some(res2),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    fn test_prove_car_cdr_invalid_tag_error_lambda() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car (lambda (x) x))"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr (lambda (x) x))"#,
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_hide_open() {
        let s = &Store::<Fr>::default();
        let expr = "(open (hide 123 456))";
        let expected = Ptr::num_u64(456);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 5, None);
    }

    #[test]
    fn test_prove_hide_wrong_secret_type() {
        let s = &Store::<Fr>::default();
        let expr = "(hide 'x 456)";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_hide_secret() {
        let s = &Store::<Fr>::default();
        let expr = "(secret (hide 123 456))";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 5, None);
    }

    #[test]
    fn test_prove_hide_open_sym() {
        let s = &Store::<Fr>::default();
        let expr = "(open (hide 123 'x))";
        let x = s.intern_user_symbol("x");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 5, None);
    }

    #[test]
    fn test_prove_commit_open_sym() {
        let s = &Store::<Fr>::default();
        let expr = "(open (commit 'x))";
        let x = s.intern_user_symbol("x");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 4, None);
    }

    #[test]
    fn test_prove_commit_open() {
        let s = &Store::<Fr>::default();
        let expr = "(open (commit 123))";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 4, None);
    }

    #[test]
    fn test_prove_commit_error() {
        let s = &Store::<Fr>::default();
        let expr = "(commit 123 456)";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_open_error() {
        let s = &Store::<Fr>::default();
        let expr = "(open 123 456)";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 1, None);
    }

    #[test]
    fn test_prove_open_wrong_type() {
        let s = &Store::<Fr>::default();
        let expr = "(open 'asdf)";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_secret_wrong_type() {
        let s = &Store::<Fr>::default();
        let expr = "(secret 'asdf)";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_commit_secret() {
        let s = &Store::<Fr>::default();
        let expr = "(secret (commit 123))";
        let expected = Ptr::num_u64(0);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 4, None);
    }

    #[test]
    fn test_prove_num() {
        let s = &Store::<Fr>::default();
        let expr = "(num 123)";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 2, None);
    }

    #[test]
    fn test_prove_num_char() {
        let s = &Store::<Fr>::default();
        let expr = r"(num #\a)";
        let expected = Ptr::num_u64(97);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 2, None);
    }

    #[test]
    fn test_prove_char_num() {
        let s = &Store::<Fr>::default();
        let expr = r#"(char 97)"#;
        let expected_a = s.read_with_default_state(r"#\a").unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            expr,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
    }

    #[test]
    fn test_prove_char_coercion() {
        let s = &Store::<Fr>::default();
        let expr = r#"(char (- 0 4294967200))"#;
        let expr2 = r#"(char (- 0 4294967199))"#;
        let expected_a = s.read_with_default_state(r"#\a").unwrap();
        let expected_b = s.read_with_default_state(r"#\b").unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            expr,
            Some(expected_a),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            expr2,
            Some(expected_b),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }

    #[test]
    fn test_prove_commit_num() {
        let s = &Store::<Fr>::default();
        let expr = "(num (commit 123))";
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 4, None);
    }

    #[test]
    fn test_prove_hide_open_comm_num() {
        let s = &Store::<Fr>::default();
        let expr = "(open (comm (num (hide 123 456))))";
        let expected = Ptr::num_u64(456);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 9, None);
    }

    #[test]
    fn test_prove_hide_secret_comm_num() {
        let s = &Store::<Fr>::default();
        let expr = "(secret (comm (num (hide 123 456))))";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 9, None);
    }

    #[test]
    fn test_prove_commit_open_comm_num() {
        let s = &Store::<Fr>::default();
        let expr = "(open (comm (num (commit 123))))";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 8, None);
    }

    #[test]
    fn test_prove_commit_secret_comm_num() {
        let s = &Store::<Fr>::default();
        let expr = "(secret (comm (num (commit 123))))";
        let expected = Ptr::num_u64(0);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 8, None);
    }

    #[test]
    fn test_prove_commit_num_open() {
        let s = &Store::<Fr>::default();
        let expr = "(open (num (commit 123)))";
        let expected = Ptr::num_u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 6, None);
    }

    #[test]
    fn test_prove_num_invalid_tag() {
        let s = &Store::<Fr>::default();
        let expr = "(num (quote x))";
        let expr1 = "(num \"asdf\")";
        let expr2 = "(num '(1))";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_comm_invalid_tag() {
        let s = &Store::<Fr>::default();
        let expr = "(comm (quote x))";
        let expr1 = "(comm \"asdf\")";
        let expr2 = "(comm '(1))";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_char_invalid_tag() {
        let s = &Store::<Fr>::default();
        let expr = "(char (quote x))";
        let expr1 = "(char \"asdf\")";
        let expr2 = "(char '(1))";
        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr1, None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 2, None);
    }

    #[test]
    fn test_prove_terminal_sym() {
        let s = &Store::<Fr>::default();
        let expr = "(quote x)";
        let x = s.intern_user_symbol("x");
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, Some(x), None, Some(terminal), None, 1, None);
    }

    #[test]
    #[should_panic]
    fn test_prove_open_opaque_commit() {
        let s = &Store::<Fr>::default();
        let expr = "(open 123)";
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
    }

    #[test]
    #[should_panic]
    fn test_prove_secret_invalid_tag() {
        let s = &Store::<Fr>::default();
        let expr = "(secret 123)";
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
    }

    #[test]
    #[should_panic]
    fn test_prove_secret_opaque_commit() {
        let s = &Store::<Fr>::default();
        let expr = "(secret (comm 123))";
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, None, None, 2, None);
    }

    #[test]
    fn test_str_car_cdr_cons() {
        let s = &Store::<Fr>::default();
        let a = s.read_with_default_state(r"#\a").unwrap();
        let apple = s.read_with_default_state(r#" "apple" "#).unwrap();
        let a_pple = s.read_with_default_state(r#" (#\a . "pple") "#).unwrap();
        let pple = s.read_with_default_state(r#" "pple" "#).unwrap();
        let empty = s.intern_string("");
        let nil = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car "apple")"#,
            Some(a),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr "apple")"#,
            Some(pple),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(car "")"#,
            Some(nil),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cdr "")"#,
            Some(empty),
            None,
            Some(terminal),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(cons #\a "pple")"#,
            Some(a_pple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(strcons #\a "pple")"#,
            Some(apple),
            None,
            Some(terminal),
            None,
            3,
            None,
        );

        test_aux::<_, _, M1<'_, _>>(
            s,
            r"(strcons #\a #\b)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(strcons "a" "b")"#,
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );

        test_aux::<_, _, M1<'_, _>>(
            s,
            r#"(strcons 1 2)"#,
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    fn relational_aux(s: &Store<Fr>, op: &str, a: &str, b: &str, res: bool) {
        let expr = &format!("({op} {a} {b})");
        let expected = if res {
            s.intern_lurk_symbol("t")
        } else {
            s.intern_nil()
        };
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(expected), None, Some(terminal), None, 3, None);
    }

    #[ignore]
    #[test]
    fn test_prove_test_relational() {
        let s = &Store::<Fr>::default();
        let lt = "<";
        let gt = ">";
        let lte = "<=";
        let gte = ">=";
        let zero = "0";
        let one = "1";
        let two = "2";

        let most_negative = &format!("{}", Num::<Fr>::most_negative());
        let most_positive = &format!("{}", Num::<Fr>::most_positive());
        let neg_one = &format!("{}", Num::<Fr>::Scalar(Fr::zero() - Fr::one()));

        relational_aux(s, lt, one, two, true);
        relational_aux(s, gt, one, two, false);
        relational_aux(s, lte, one, two, true);
        relational_aux(s, gte, one, two, false);

        relational_aux(s, lt, two, one, false);
        relational_aux(s, gt, two, one, true);
        relational_aux(s, lte, two, one, false);
        relational_aux(s, gte, two, one, true);

        relational_aux(s, lt, one, one, false);
        relational_aux(s, gt, one, one, false);
        relational_aux(s, lte, one, one, true);
        relational_aux(s, gte, one, one, true);

        relational_aux(s, lt, zero, two, true);
        relational_aux(s, gt, zero, two, false);
        relational_aux(s, lte, zero, two, true);
        relational_aux(s, gte, zero, two, false);

        relational_aux(s, lt, two, zero, false);
        relational_aux(s, gt, two, zero, true);
        relational_aux(s, lte, two, zero, false);
        relational_aux(s, gte, two, zero, true);

        relational_aux(s, lt, zero, zero, false);
        relational_aux(s, gt, zero, zero, false);
        relational_aux(s, lte, zero, zero, true);
        relational_aux(s, gte, zero, zero, true);

        relational_aux(s, lt, most_negative, zero, true);
        relational_aux(s, gt, most_negative, zero, false);
        relational_aux(s, lte, most_negative, zero, true);
        relational_aux(s, gte, most_negative, zero, false);

        relational_aux(s, lt, zero, most_negative, false);
        relational_aux(s, gt, zero, most_negative, true);
        relational_aux(s, lte, zero, most_negative, false);
        relational_aux(s, gte, zero, most_negative, true);

        relational_aux(s, lt, most_negative, most_positive, true);
        relational_aux(s, gt, most_negative, most_positive, false);
        relational_aux(s, lte, most_negative, most_positive, true);
        relational_aux(s, gte, most_negative, most_positive, false);

        relational_aux(s, lt, most_positive, most_negative, false);
        relational_aux(s, gt, most_positive, most_negative, true);
        relational_aux(s, lte, most_positive, most_negative, false);
        relational_aux(s, gte, most_positive, most_negative, true);

        relational_aux(s, lt, most_negative, most_negative, false);
        relational_aux(s, gt, most_negative, most_negative, false);
        relational_aux(s, lte, most_negative, most_negative, true);
        relational_aux(s, gte, most_negative, most_negative, true);

        relational_aux(s, lt, one, most_positive, true);
        relational_aux(s, gt, one, most_positive, false);
        relational_aux(s, lte, one, most_positive, true);
        relational_aux(s, gte, one, most_positive, false);

        relational_aux(s, lt, most_positive, one, false);
        relational_aux(s, gt, most_positive, one, true);
        relational_aux(s, lte, most_positive, one, false);
        relational_aux(s, gte, most_positive, one, true);

        relational_aux(s, lt, one, most_negative, false);
        relational_aux(s, gt, one, most_negative, true);
        relational_aux(s, lte, one, most_negative, false);
        relational_aux(s, gte, one, most_negative, true);

        relational_aux(s, lt, most_negative, one, true);
        relational_aux(s, gt, most_negative, one, false);
        relational_aux(s, lte, most_negative, one, true);
        relational_aux(s, gte, most_negative, one, false);

        relational_aux(s, lt, neg_one, most_positive, true);
        relational_aux(s, gt, neg_one, most_positive, false);
        relational_aux(s, lte, neg_one, most_positive, true);
        relational_aux(s, gte, neg_one, most_positive, false);

        relational_aux(s, lt, most_positive, neg_one, false);
        relational_aux(s, gt, most_positive, neg_one, true);
        relational_aux(s, lte, most_positive, neg_one, false);
        relational_aux(s, gte, most_positive, neg_one, true);

        relational_aux(s, lt, neg_one, most_negative, false);
        relational_aux(s, gt, neg_one, most_negative, true);
        relational_aux(s, lte, neg_one, most_negative, false);
        relational_aux(s, gte, neg_one, most_negative, true);

        relational_aux(s, lt, most_negative, neg_one, true);
        relational_aux(s, gt, most_negative, neg_one, false);
        relational_aux(s, lte, most_negative, neg_one, true);
        relational_aux(s, gte, most_negative, neg_one, false);
    }

    #[test]
    fn test_relational_edge_case_identity() {
        let s = &Store::<Fr>::default();
        // Normally, a value cannot be less than the result of incrementing it.
        // However, the most positive field element (when viewed as signed)
        // is the exception. Incrementing it yields the most negative element,
        // which is less than the most positive.
        let expr = "(let ((most-positive (/ (- 0 1) 2))
                          (most-negative (+ 1 most-positive)))
                      (< most-negative most-positive))";
        let t = s.intern_lurk_symbol("t");
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 19, None);
    }

    #[test]
    fn test_prove_test_eval() {
        let s = &Store::<Fr>::default();
        let expr = "(* 3 (eval  (cons '+ (cons 1 (cons 2 nil)))))";
        let expr2 = "(* 5 (eval '(+ 1 a) '((a . 3))))"; // two-arg eval, optional second arg is env.
        let res = Ptr::num_u64(9);
        let res2 = Ptr::num_u64(20);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 17, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 9, None);
    }

    #[test]
    fn test_prove_test_keyword() {
        let s = &Store::<Fr>::default();

        let expr = ":asdf";
        let expr2 = "(eq :asdf :asdf)";
        let expr3 = "(eq :asdf 'asdf)";
        let res = s.key("asdf");
        let res2 = s.intern_lurk_symbol("t");
        let res3 = s.intern_nil();

        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 1, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res3), None, Some(terminal), None, 3, None);
    }

    // The following functional commitment tests were discovered to fail. They are commented out (as tests) for now so
    // they can be addressed independently in future work.

    #[test]
    fn test_prove_functional_commitment() {
        let s = &Store::<Fr>::default();

        let expr = "(let ((f (commit (let ((num 9)) (lambda (f) (f num)))))
                          (inc (lambda (x) (+ x 1))))
                      ((open f) inc))";
        let res = Ptr::num_u64(10);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 25, None);
    }

    #[test]
    #[ignore]
    fn test_prove_complicated_functional_commitment() {
        let s = &Store::<Fr>::default();

        let expr = "(let ((f (commit (let ((nums '(1 2 3))) (lambda (f) (f nums)))))
                          (in (letrec ((sum-aux (lambda (acc nums)
                                              (if nums
                                                (sum-aux (+ acc (car nums)) (cdr nums))
                                                acc)))
                                   (sum (sum-aux 0)))
                             (lambda (nums)
                               (sum nums)))))

                      ((open f) in))";
        let res = Ptr::num_u64(6);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 108, None);
    }

    #[test]
    fn test_prove_test_fold_cons_regression() {
        let s = &Store::<Fr>::default();
        let expr = "(letrec ((fold (lambda (op acc l)
                                     (if l
                                         (fold op (op acc (car l)) (cdr l))
                                         acc))))
                      (fold (lambda (x y) (+ x y)) 0 '(1 2 3)))";
        let res = Ptr::num_u64(6);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 152, None);
    }

    #[test]
    fn test_prove_test_lambda_args_regression() {
        let s = &Store::<Fr>::default();

        let expr = "(cons (lambda (x y) nil) nil)";
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_reduce_sym_contradiction_regression() {
        let s = &Store::<Fr>::default();

        let expr = "(eval 'a '(nil))";
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 4, None);
    }

    #[test]
    fn test_prove_test_self_eval_env_not_nil() {
        let s = &Store::<Fr>::default();

        // NOTE: cond1 shouldn't depend on env-is-not-nil
        // therefore this unit test is not very useful
        // the conclusion is that by removing condition env-is-not-nil from cond1,
        // we solve this soundness problem
        // this solution makes the circuit a bit smaller
        let expr = "(let ((a 1)) t)";

        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_test_self_eval_nil() {
        let s = &Store::<Fr>::default();

        // nil doesn't have SYM tag
        let expr = "nil";

        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(terminal), None, 1, None);
    }

    #[test]
    fn test_prove_test_env_not_nil_and_binding_nil() {
        let s = &Store::<Fr>::default();

        let expr = "(let ((a 1) (b 2)) c)";

        let error = Ptr::null(Tag::Cont(Error));
        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 7, None);
    }

    #[test]
    fn test_prove_test_eval_bad_form() {
        let s = &Store::<Fr>::default();
        let expr = "(* 5 (eval '(+ 1 a) '((0 . 3))))"; // two-arg eval, optional second arg is env. This tests for error on malformed env.
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 8, None);
    }

    #[test]
    fn test_prove_test_u64_self_evaluating() {
        let s = &Store::<Fr>::default();

        let expr = "123u64";
        let res = Ptr::u64(123);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 1, None);
    }

    #[test]
    fn test_prove_test_u64_mul() {
        let s = &Store::<Fr>::default();

        let expr = "(* (u64 18446744073709551615) (u64 2))";
        let expr2 = "(* 18446744073709551615u64 2u64)";
        let expr3 = "(* (- 0u64 1u64) 2u64)";
        let expr4 = "(u64 18446744073709551617)";
        let res = Ptr::u64(18446744073709551614);
        let res2 = Ptr::u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 7, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res), None, Some(terminal), None, 6, None);
        test_aux::<_, _, M1<'_, _>>(s, expr4, Some(res2), None, Some(terminal), None, 2, None);
    }

    #[test]
    fn test_prove_test_u64_add() {
        let s = &Store::<Fr>::default();

        let expr = "(+ 18446744073709551615u64 2u64)";
        let expr2 = "(+ (- 0u64 1u64) 2u64)";
        let res = Ptr::u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 6, None);
    }

    #[test]
    fn test_prove_test_u64_sub() {
        let s = &Store::<Fr>::default();

        let expr = "(- 2u64 1u64)";
        let expr2 = "(- 0u64 1u64)";
        let expr3 = "(+ 1u64 (- 0u64 1u64))";
        let res = Ptr::u64(1);
        let res2 = Ptr::u64(18446744073709551615);
        let res3 = Ptr::u64(0);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res3), None, Some(terminal), None, 6, None);
    }

    #[test]
    fn test_prove_test_u64_div() {
        let s = &Store::<Fr>::default();

        let expr = "(/ 100u64 2u64)";
        let res = Ptr::u64(50);

        let expr2 = "(/ 100u64 3u64)";
        let res2 = Ptr::u64(33);

        let expr3 = "(/ 100u64 0u64)";

        let terminal = Ptr::null(Tag::Cont(Terminal));
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_test_u64_mod() {
        let s = &Store::<Fr>::default();

        let expr = "(% 100u64 2u64)";
        let res = Ptr::u64(0);

        let expr2 = "(% 100u64 3u64)";
        let res2 = Ptr::u64(1);

        let expr3 = "(% 100u64 0u64)";

        let terminal = Ptr::null(Tag::Cont(Terminal));
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_test_num_mod() {
        let s = &Store::<Fr>::default();

        let expr = "(% 100 3)";
        let expr2 = "(% 100 3u64)";
        let expr3 = "(% 100u64 3)";

        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, None, None, Some(error), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_test_u64_comp() {
        let s = &Store::<Fr>::default();

        let expr = "(< 0u64 1u64)";
        let expr2 = "(< 1u64 0u64)";
        let expr3 = "(<= 0u64 1u64)";
        let expr4 = "(<= 1u64 0u64)";

        let expr5 = "(> 0u64 1u64)";
        let expr6 = "(> 1u64 0u64)";
        let expr7 = "(>= 0u64 1u64)";
        let expr8 = "(>= 1u64 0u64)";

        let expr9 = "(<= 0u64 0u64)";
        let expr10 = "(>= 0u64 0u64)";

        let t = s.intern_lurk_symbol("t");
        let nil = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(nil), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, Some(t), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr4, Some(nil), None, Some(terminal), None, 3, None);

        test_aux::<_, _, M1<'_, _>>(s, expr5, Some(nil), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr6, Some(t), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr7, Some(nil), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr8, Some(t), None, Some(terminal), None, 3, None);

        test_aux::<_, _, M1<'_, _>>(s, expr9, Some(t), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr10, Some(t), None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_test_u64_conversion() {
        let s = &Store::<Fr>::default();

        let expr = "(+ 0 1u64)";
        let expr2 = "(num 1u64)";
        let expr3 = "(+ 1 1u64)";
        let expr4 = "(u64 (+ 1 1))";
        let res = Ptr::num_u64(1);
        let res2 = Ptr::num_u64(2);
        let res3 = Ptr::u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res), None, Some(terminal), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(s, expr3, Some(res2), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr4, Some(res3), None, Some(terminal), None, 5, None);
    }

    #[test]
    fn test_prove_test_u64_num_comparison() {
        let s = &Store::<Fr>::default();

        let expr = "(= 1 1u64)";
        let expr2 = "(= 1 2u64)";
        let t = s.intern_lurk_symbol("t");
        let nil = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(t), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(nil), None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_test_u64_num_cons() {
        let s = &Store::<Fr>::default();

        let expr = "(cons 1 1u64)";
        let expr2 = "(cons 1u64 1)";
        let res = s.read_with_default_state("(1 . 1u64)").unwrap();
        let res2 = s.read_with_default_state("(1u64 . 1)").unwrap();
        let terminal = Ptr::null(Tag::Cont(Terminal));

        test_aux::<_, _, M1<'_, _>>(s, expr, Some(res), None, Some(terminal), None, 3, None);
        test_aux::<_, _, M1<'_, _>>(s, expr2, Some(res2), None, Some(terminal), None, 3, None);
    }

    #[test]
    fn test_prove_test_hide_u64_secret() {
        let s = &Store::<Fr>::default();

        let expr = "(hide 0u64 123)";
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_test_mod_by_zero_error() {
        let s = &Store::<Fr>::default();

        let expr = "(% 0 0)";
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_dotted_syntax_error() {
        let s = &Store::<Fr>::default();
        let expr = "(let ((a (lambda (x) (+ x 1)))) (a . 1))";
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, expr, None, None, Some(error), None, 3, None);
    }

    #[test]
    fn test_prove_call_literal_fun() {
        let s = &Store::<Fr>::default();
        let empty_env = s.intern_nil();
        let arg = s.intern_user_symbol("x");
        let body = s.read_with_default_state("((+ x 1))").unwrap();
        let fun = s.intern_3_ptrs(Tag::Expr(ExprTag::Fun), arg, body, empty_env);
        let input = Ptr::num_u64(9);
        let expr = s.list(vec![fun, input]);
        let res = Ptr::num_u64(10);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        let lang: Arc<Lang<Fr, Coproc<Fr>>> = Arc::new(Lang::new());

        nova_test_full_aux2::<_, _, M1<'_, _>>(
            s,
            expr,
            Some(res),
            None,
            Some(terminal),
            None,
            7,
            DEFAULT_REDUCTION_COUNT,
            false,
            None,
            lang,
        );
    }

    #[test]
    fn test_prove_lambda_body_syntax() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));

        test_aux::<_, _, M1<'_, _>>(s, "((lambda ()))", None, None, Some(error), None, 2, None);
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda () 1 2))",
            None,
            None,
            Some(error),
            None,
            2,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x)) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x) 1 2) 1)",
            None,
            None,
            Some(error),
            None,
            3,
            None,
        );
    }

    #[test]
    #[ignore]
    fn test_prove_non_symbol_binding_error() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));

        let test = |x| {
            let expr = format!("(let (({x} 123)) {x})");
            let expr2 = format!("(letrec (({x} 123)) {x})");
            let expr3 = format!("(lambda ({x}) {x})");

            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
            test_aux::<_, _, M1<'_, _>>(s, &expr2, None, None, Some(error), None, 1, None);
            test_aux::<_, _, M1<'_, _>>(s, &expr3, None, None, Some(error), None, 1, None);
        };

        test(":a");
        test("1");
        test("\"string\"");
        test("1u64");
        test("#\\x");
    }

    #[test]
    fn test_prove_head_with_sym_mimicking_value() {
        let s = &Store::<Fr>::default();
        let error = Ptr::null(Tag::Cont(Error));

        let hash_num = |s: &Store<Fr>, state: Rc<RefCell<State>>, name| {
            let sym = s.read(state, name).unwrap();
            let z_ptr = s.hash_ptr(&sym).unwrap();
            let hash = *z_ptr.value();
            Num::Scalar(hash)
        };

        let state = State::init_lurk_state().rccell();
        {
            // binop
            let expr = format!("({} 1 1)", hash_num(s, state.clone(), "+"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // unop
            let expr = format!("({} '(1 . 2))", hash_num(s, state.clone(), "car"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // let_or_letrec
            let expr = format!("({} ((a 1)) a)", hash_num(s, state.clone(), "let"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // current-env
            let expr = format!("({})", hash_num(s, state.clone(), "current-env"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // lambda
            let expr = format!("({} (x) 123)", hash_num(s, state.clone(), "lambda"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // quote
            let expr = format!("({} asdf)", hash_num(s, state.clone(), "quote"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
        {
            // if
            let expr = format!("({} t 123 456)", hash_num(s, state, "if"));
            test_aux::<_, _, M1<'_, _>>(s, &expr, None, None, Some(error), None, 1, None);
        }
    }

    #[test]
    fn test_dumb_lang() {
        use crate::coprocessor::test::DumbCoprocessor;

        let s = &Store::<Fr>::default();

        let mut lang = Lang::<Fr, DumbCoprocessor<Fr>>::new();
        let name = user_sym("cproc-dumb");
        let dumb = DumbCoprocessor::new();

        lang.add_coprocessor_lem(name, dumb, s);

        // 9^2 + 8 = 89
        let expr = "(cproc-dumb 9 8)";

        // The dumb coprocessor cannot be shadowed.
        let expr2 = "(let ((cproc-dumb (lambda (a b) (* a b))))
                   (cproc-dumb 9 8))";

        let expr3 = "(cproc-dumb 9 8 123)";
        let expr4 = "(cproc-dumb 9)";

        let res = Ptr::num_u64(89);
        let error = Ptr::null(Tag::Cont(Error));
        let lang = Arc::new(lang);

        test_aux::<_, _, C1LEM<'_, _, DumbCoprocessor<_>>>(
            s,
            expr,
            Some(res),
            None,
            None,
            None,
            3,
            Some(lang.clone()),
        );
        test_aux::<_, _, C1LEM<'_, _, DumbCoprocessor<_>>>(
            s,
            expr2,
            Some(res),
            None,
            None,
            None,
            6,
            Some(lang.clone()),
        );
        test_aux::<_, _, C1LEM<'_, _, DumbCoprocessor<_>>>(
            s,
            expr3,
            None,
            None,
            Some(error),
            None,
            4,
            Some(lang.clone()),
        );
        test_aux::<_, _, C1LEM<'_, _, DumbCoprocessor<_>>>(
            s,
            expr4,
            None,
            None,
            Some(error),
            None,
            2,
            Some(lang),
        );
    }

    // This is related to issue #426
    #[test]
    fn test_prove_lambda_body_nil() {
        let s = &Store::<Fr>::default();
        let expected = s.intern_nil();
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "((lambda (x) nil) 0)",
            Some(expected),
            None,
            Some(terminal),
            None,
            4,
            None,
        );
    }

    // The following 3 tests are related to issue #424
    #[test]
    fn test_letrec_let_nesting() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(2);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((x (let ((z 0)) 1))) 2)",
            Some(expected),
            None,
            Some(terminal),
            None,
            6,
            None,
        );
    }
    #[test]
    fn test_let_sequencing() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(1);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(let ((x 0) (y x)) 1)",
            Some(expected),
            None,
            Some(terminal),
            None,
            5,
            None,
        );
    }
    #[test]
    fn test_letrec_sequencing() {
        let s = &Store::<Fr>::default();
        let expected = Ptr::num_u64(3);
        let terminal = Ptr::null(Tag::Cont(Terminal));
        test_aux::<_, _, M1<'_, _>>(
            s,
            "(letrec ((x 0) (y (letrec ((inner 1)) 2))) 3)",
            Some(expected),
            None,
            Some(terminal),
            None,
            8,
            None,
        );
    }
}
