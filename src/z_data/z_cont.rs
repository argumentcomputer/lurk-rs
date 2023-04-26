use anyhow::anyhow;

use crate::field::LurkField;
use crate::z_data::Encodable;
use crate::z_data::ZData;

#[cfg(not(target_arch = "wasm32"))]
use proptest::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use proptest_derive::Arbitrary;

use crate::hash::PoseidonCache;
use crate::tag::ContTag;
use crate::tag::Op1;
use crate::tag::Op2;
use crate::tag::Tag;
use crate::z_data::{ZContPtr, ZExprPtr, ZPtr};

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Arbitrary))]
#[cfg_attr(not(target_arch = "wasm32"), proptest(no_bound))]
pub enum ZCont<F: LurkField> {
    Outermost,
    Call {
        unevaled_arg: ZExprPtr<F>,
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Call2 {
        function: ZExprPtr<F>,
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Tail {
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Error,
    Lookup {
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Unop {
        operator: Op1,
        continuation: ZContPtr<F>,
    },
    Binop {
        operator: Op2,
        saved_env: ZExprPtr<F>,
        unevaled_args: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Binop2 {
        operator: Op2,
        evaled_arg: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    If {
        unevaled_args: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Let {
        var: ZExprPtr<F>,
        body: ZExprPtr<F>,
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    LetRec {
        var: ZExprPtr<F>,
        body: ZExprPtr<F>,
        saved_env: ZExprPtr<F>,
        continuation: ZContPtr<F>,
    },
    Emit {
        continuation: ZContPtr<F>,
    },
    Dummy,
    Terminal,
}
impl<F: LurkField> ZCont<F> {
    pub fn z_ptr(&self, cache: PoseidonCache<F>) -> ZContPtr<F> {
        match self {
            Self::Outermost => {
                // TODO: replace hash8 of F::zero with digest F::zero()
                let hash = cache.hash8(&[F::zero(); 8]);
                ZPtr(ContTag::Outermost, hash)
            }
            Self::Call {
                unevaled_arg,
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    unevaled_arg.0.to_field(),
                    unevaled_arg.1,
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Call, hash)
            }
            Self::Call2 {
                function,
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    function.0.to_field(),
                    function.1,
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Call2, hash)
            }
            Self::Tail {
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Tail, hash)
            }
            Self::Error => {
                // TODO: replace hash8 of F::zero with digest F::zero()
                let hash = cache.hash8(&[F::zero(); 8]);
                ZPtr(ContTag::Error, hash)
            }
            Self::Lookup {
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Lookup, hash)
            }
            Self::Unop {
                operator,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    operator.to_field(),
                    F::zero(),
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Unop, hash)
            }
            Self::Binop {
                operator,
                saved_env,
                unevaled_args,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    operator.to_field(),
                    F::zero(),
                    saved_env.0.to_field(),
                    saved_env.1,
                    unevaled_args.0.to_field(),
                    unevaled_args.1,
                    continuation.0.to_field(),
                    continuation.1,
                ]);
                ZPtr(ContTag::Binop, hash)
            }
            Self::Binop2 {
                operator,
                evaled_arg,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    operator.to_field(),
                    F::zero(),
                    evaled_arg.0.to_field(),
                    evaled_arg.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::Binop2, hash)
            }
            Self::If {
                unevaled_args,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    unevaled_args.0.to_field(),
                    unevaled_args.1,
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::If, hash)
            }
            Self::Let {
                var,
                body,
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    var.0.to_field(),
                    var.1,
                    body.0.to_field(),
                    body.1,
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                ]);
                ZPtr(ContTag::Let, hash)
            }
            Self::LetRec {
                var,
                body,
                saved_env,
                continuation,
            } => {
                let hash = cache.hash8(&[
                    var.0.to_field(),
                    var.1,
                    body.0.to_field(),
                    body.1,
                    saved_env.0.to_field(),
                    saved_env.1,
                    continuation.0.to_field(),
                    continuation.1,
                ]);
                ZPtr(ContTag::Lookup, hash)
            }
            Self::Emit { continuation } => {
                let hash = cache.hash8(&[
                    continuation.0.to_field(),
                    continuation.1,
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                    F::zero(),
                ]);
                ZPtr(ContTag::LetRec, hash)
            }
            Self::Dummy => {
                // TODO: replace hash8 of F::zero with digest F::zero()
                let hash = cache.hash8(&[F::zero(); 8]);
                ZPtr(ContTag::Dummy, hash)
            }
            Self::Terminal => {
                // TODO: replace hash8 of F::zero with digest F::zero()
                let hash = cache.hash8(&[F::zero(); 8]);
                ZPtr(ContTag::Terminal, hash)
            }
        }
    }
}

impl<F: LurkField> Encodable for ZCont<F> {
    fn ser(&self) -> ZData {
        match self {
            ZCont::Outermost => ZData::Cell(vec![ZData::Atom(vec![0u8])]),
            ZCont::Call {
                unevaled_arg,
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![1u8]),
                unevaled_arg.ser(),
                saved_env.ser(),
                continuation.ser(),
            ]),
            ZCont::Call2 {
                function,
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![2u8]),
                function.ser(),
                saved_env.ser(),
                continuation.ser(),
            ]),
            ZCont::Tail {
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![3u8]),
                saved_env.ser(),
                continuation.ser(),
            ]),
            ZCont::Error => ZData::Cell(vec![ZData::Atom(vec![4u8])]),
            ZCont::Lookup {
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![5u8]),
                saved_env.ser(),
                continuation.ser(),
            ]),

            ZCont::Unop {
                operator,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![6u8]),
                operator.ser(),
                continuation.ser(),
            ]),
            ZCont::Binop {
                operator,
                saved_env,
                unevaled_args,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![7u8]),
                operator.ser(),
                saved_env.ser(),
                unevaled_args.ser(),
                continuation.ser(),
            ]),
            ZCont::Binop2 {
                operator,
                evaled_arg,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![8u8]),
                operator.ser(),
                evaled_arg.ser(),
                continuation.ser(),
            ]),
            ZCont::If {
                unevaled_args,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![9u8]),
                unevaled_args.ser(),
                continuation.ser(),
            ]),
            ZCont::Let {
                var,
                body,
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![10u8]),
                var.ser(),
                body.ser(),
                saved_env.ser(),
                continuation.ser(),
            ]),
            ZCont::LetRec {
                var,
                body,
                saved_env,
                continuation,
            } => ZData::Cell(vec![
                ZData::Atom(vec![11u8]),
                var.ser(),
                body.ser(),
                saved_env.ser(),
                continuation.ser(),
            ]),
            ZCont::Emit { continuation } => {
                ZData::Cell(vec![ZData::Atom(vec![12u8]), continuation.ser()])
            }
            ZCont::Dummy => ZData::Cell(vec![ZData::Atom(vec![13u8])]),
            ZCont::Terminal => ZData::Cell(vec![ZData::Atom(vec![14u8])]),
        }
    }
    fn de(ld: &ZData) -> anyhow::Result<Self> {
        match ld {
            ZData::Atom(v) => Err(anyhow!("ZExpr::Atom({:?})", v)),
            ZData::Cell(v) => match (*v).as_slice() {
                [ZData::Atom(u)] if *u == vec![0u8] => Ok(ZCont::Outermost),
                [ZData::Atom(u), x, y, z] if *u == vec![1u8] => Ok(ZCont::Call {
                    unevaled_arg: ZExprPtr::de(x)?,
                    saved_env: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), x, y, z] if *u == vec![2u8] => Ok(ZCont::Call2 {
                    function: ZExprPtr::de(x)?,
                    saved_env: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), x, y] if *u == vec![3u8] => Ok(ZCont::Tail {
                    saved_env: ZExprPtr::de(x)?,
                    continuation: ZContPtr::de(y)?,
                }),
                [ZData::Atom(u)] if *u == vec![4u8] => Ok(ZCont::Error),
                [ZData::Atom(u), x, y] if *u == vec![5u8] => Ok(ZCont::Lookup {
                    saved_env: ZExprPtr::de(x)?,
                    continuation: ZContPtr::de(y)?,
                }),
                [ZData::Atom(u), x, y] if *u == vec![6u8] => Ok(ZCont::Unop {
                    operator: Op1::de(x)?,
                    continuation: ZContPtr::de(y)?,
                }),
                [ZData::Atom(u), w, x, y, z] if *u == vec![7u8] => Ok(ZCont::Binop {
                    operator: Op2::de(w)?,
                    saved_env: ZExprPtr::de(x)?,
                    unevaled_args: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), x, y, z] if *u == vec![8u8] => Ok(ZCont::Binop2 {
                    operator: Op2::de(x)?,
                    evaled_arg: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), x, y] if *u == vec![9u8] => Ok(ZCont::If {
                    unevaled_args: ZExprPtr::de(x)?,
                    continuation: ZContPtr::de(y)?,
                }),
                [ZData::Atom(u), w, x, y, z] if *u == vec![10u8] => Ok(ZCont::Let {
                    var: ZExprPtr::de(w)?,
                    body: ZExprPtr::de(x)?,
                    saved_env: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), w, x, y, z] if *u == vec![11u8] => Ok(ZCont::LetRec {
                    var: ZExprPtr::de(w)?,
                    body: ZExprPtr::de(x)?,
                    saved_env: ZExprPtr::de(y)?,
                    continuation: ZContPtr::de(z)?,
                }),
                [ZData::Atom(u), x] if *u == vec![12u8] => Ok(ZCont::Emit {
                    continuation: ZContPtr::de(x)?,
                }),
                [ZData::Atom(u)] if *u == vec![13u8] => Ok(ZCont::Dummy),
                [ZData::Atom(u)] if *u == vec![14u8] => Ok(ZCont::Terminal),
                _ => Err(anyhow!("ZExpr::Cell({:?})", v)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pasta_curves::pallas::Scalar;

    proptest! {
          #[test]
          fn prop_z_cont(x in any::<ZCont<Scalar>>()) {
              let ser = x.ser();
              let de  = ZCont::de(&ser).expect("read ZCont");
              assert_eq!(x, de)
          }
    }
}
