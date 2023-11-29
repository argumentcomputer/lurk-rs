(function() {var implementors = {
"lurk":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;SynthesisError&gt; for <a class=\"enum\" href=\"lurk/error/enum.ProofError.html\" title=\"enum lurk::error::ProofError\">ProofError</a>"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>, C: <a class=\"trait\" href=\"lurk/coprocessor/trait.Coprocessor.html\" title=\"trait lurk::coprocessor::Coprocessor\">Coprocessor</a>&lt;F&gt;, S: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.Into.html\" title=\"trait core::convert::Into\">Into</a>&lt;<a class=\"struct\" href=\"lurk/struct.Symbol.html\" title=\"struct lurk::Symbol\">Symbol</a>&gt;&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.tuple.html\">(S, C)</a>&gt; for <a class=\"struct\" href=\"lurk/eval/lang/struct.Binding.html\" title=\"struct lurk::eval::lang::Binding\">Binding</a>&lt;F, C&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/store/struct.Error.html\" title=\"struct lurk::store::Error\">Error</a>&gt; for <a class=\"enum\" href=\"lurk/error/enum.ProofError.html\" title=\"enum lurk::error::ProofError\">ProofError</a>"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"lurk/enum.UInt.html\" title=\"enum lurk::UInt\">UInt</a>&gt; for <a class=\"enum\" href=\"lurk/enum.Num.html\" title=\"enum lurk::Num\">Num</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/coprocessor/trie/struct.InsertCoprocessor.html\" title=\"struct lurk::coprocessor::trie::InsertCoprocessor\">InsertCoprocessor</a>&lt;F&gt;&gt; for <a class=\"enum\" href=\"lurk/coprocessor/trie/enum.TrieCoproc.html\" title=\"enum lurk::coprocessor::trie::TrieCoproc\">TrieCoproc</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/circuit/gadgets/pointer/struct.AllocatedPtr.html\" title=\"struct lurk::circuit::gadgets::pointer::AllocatedPtr\">AllocatedPtr</a>&lt;F&gt;&gt; for <a class=\"struct\" href=\"lurk/circuit/gadgets/pointer/struct.AllocatedContPtr.html\" title=\"struct lurk::circuit::gadgets::pointer::AllocatedContPtr\">AllocatedContPtr</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a>&gt; for <a class=\"enum\" href=\"lurk/enum.Num.html\" title=\"enum lurk::Num\">Num</a>&lt;F&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"lurk/lem/enum.Tag.html\" title=\"enum lurk::lem::Tag\">Tag</a>&gt; for <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u16.html\">u16</a>"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/coprocessor/sha256/struct.Sha256Coprocessor.html\" title=\"struct lurk::coprocessor::sha256::Sha256Coprocessor\">Sha256Coprocessor</a>&lt;F&gt;&gt; for <a class=\"enum\" href=\"lurk/coprocessor/sha256/enum.Sha256Coproc.html\" title=\"enum lurk::coprocessor::sha256::Sha256Coproc\">Sha256Coproc</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/coprocessor/trie/struct.NewCoprocessor.html\" title=\"struct lurk::coprocessor::trie::NewCoprocessor\">NewCoprocessor</a>&lt;F&gt;&gt; for <a class=\"enum\" href=\"lurk/coprocessor/trie/enum.TrieCoproc.html\" title=\"enum lurk::coprocessor::trie::TrieCoproc\">TrieCoproc</a>&lt;F&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;&amp;'static <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.str.html\">str</a>&gt; for <a class=\"struct\" href=\"lurk/struct.Symbol.html\" title=\"struct lurk::Symbol\">Symbol</a>"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/circuit/gadgets/pointer/struct.AllocatedContPtr.html\" title=\"struct lurk::circuit::gadgets::pointer::AllocatedContPtr\">AllocatedContPtr</a>&lt;F&gt;&gt; for <a class=\"struct\" href=\"lurk/circuit/gadgets/pointer/struct.AllocatedPtr.html\" title=\"struct lurk::circuit::gadgets::pointer::AllocatedPtr\">AllocatedPtr</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/coprocessor/trie/struct.LookupCoprocessor.html\" title=\"struct lurk::coprocessor::trie::LookupCoprocessor\">LookupCoprocessor</a>&lt;F&gt;&gt; for <a class=\"enum\" href=\"lurk/coprocessor/trie/enum.TrieCoproc.html\" title=\"enum lurk::coprocessor::trie::TrieCoproc\">TrieCoproc</a>&lt;F&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;NovaError&gt; for <a class=\"enum\" href=\"lurk/error/enum.ProofError.html\" title=\"enum lurk::error::ProofError\">ProofError</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/store/struct.Error.html\" title=\"struct lurk::store::Error\">Error</a>&gt; for <a class=\"enum\" href=\"lurk/error/enum.ReductionError.html\" title=\"enum lurk::error::ReductionError\">ReductionError</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"lurk/enum.UInt.html\" title=\"enum lurk::UInt\">UInt</a>&gt; for <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a>"],["impl&lt;F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/eval/lang/struct.DummyCoprocessor.html\" title=\"struct lurk::eval::lang::DummyCoprocessor\">DummyCoprocessor</a>&lt;F&gt;&gt; for <a class=\"enum\" href=\"lurk/eval/lang/enum.Coproc.html\" title=\"enum lurk::eval::lang::Coproc\">Coproc</a>&lt;F&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"lurk/error/enum.ReductionError.html\" title=\"enum lurk::error::ReductionError\">ReductionError</a>&gt; for <a class=\"enum\" href=\"lurk/error/enum.ProofError.html\" title=\"enum lurk::error::ProofError\">ProofError</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a>&gt; for <a class=\"enum\" href=\"lurk/enum.UInt.html\" title=\"enum lurk::UInt\">UInt</a>"]]
};if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()