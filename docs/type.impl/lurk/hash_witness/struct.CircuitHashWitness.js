(function() {var type_impls = {
"lurk":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#362-397\">source</a><a href=\"#impl-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;<span class=\"where fmt-newline\">where\n    T::<a class=\"associatedtype\" href=\"lurk/hash_witness/trait.ContentAddressed.html#associatedtype.ScalarPtrRepr\" title=\"type lurk::hash_witness::ContentAddressed::ScalarPtrRepr\">ScalarPtrRepr</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a>,</span></h3></section></summary><div class=\"impl-items\"><section id=\"method.names_and_ptrs\" class=\"method\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#367-375\">source</a><h4 class=\"code-header\">pub fn <a href=\"lurk/hash_witness/struct.CircuitHashWitness.html#tymethod.names_and_ptrs\" class=\"fn\">names_and_ptrs</a>(\n    &amp;self,\n    s: &amp;<a class=\"struct\" href=\"lurk/store/struct.Store.html\" title=\"struct lurk::store::Store\">Store</a>&lt;F&gt;\n) -&gt; &amp;<a class=\"struct\" href=\"https://doc.rust-lang.org/nightly/alloc/vec/struct.Vec.html\" title=\"struct alloc::vec::Vec\">Vec</a>&lt;(Name, <a class=\"enum\" href=\"https://doc.rust-lang.org/nightly/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;T::<a class=\"associatedtype\" href=\"lurk/hash_witness/trait.ContentAddressed.html#associatedtype.ScalarPtrRepr\" title=\"type lurk::hash_witness::ContentAddressed::ScalarPtrRepr\">ScalarPtrRepr</a>&gt;)&gt;</h4></section><details class=\"toggle method-toggle\" open><summary><section id=\"method.circuit_witness_blocks\" class=\"method\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#378-396\">source</a><h4 class=\"code-header\">pub fn <a href=\"lurk/hash_witness/struct.CircuitHashWitness.html#tymethod.circuit_witness_blocks\" class=\"fn\">circuit_witness_blocks</a>(\n    &amp;self,\n    s: &amp;<a class=\"struct\" href=\"lurk/store/struct.Store.html\" title=\"struct lurk::store::Store\">Store</a>&lt;F&gt;,\n    hash_constants: &amp;<a class=\"enum\" href=\"lurk/hash/enum.HashConst.html\" title=\"enum lurk::hash::HashConst\">HashConst</a>&lt;'_, F&gt;\n) -&gt; &amp;<a class=\"type\" href=\"lurk/hash_witness/type.HashCircuitWitnessBlocks.html\" title=\"type lurk::hash_witness::HashCircuitWitnessBlocks\">HashCircuitWitnessBlocks</a>&lt;F&gt;</h4></section></summary><div class=\"docblock\"><p>Precompute the witness blocks for all the named hashes.</p>\n</div></details></div></details>",0,"lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"],["<section id=\"impl-StructuralPartialEq-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#impl-StructuralPartialEq-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.StructuralPartialEq.html\" title=\"trait core::marker::StructuralPartialEq\">StructuralPartialEq</a> for <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;</h3></section>","StructuralPartialEq","lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PartialEq-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#impl-PartialEq-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> + <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> for <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;<span class=\"where fmt-newline\">where\n    T::<a class=\"associatedtype\" href=\"lurk/hash_witness/trait.ContentAddressed.html#associatedtype.ScalarPtrRepr\" title=\"type lurk::hash_witness::ContentAddressed::ScalarPtrRepr\">ScalarPtrRepr</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a>,</span></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.eq\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#method.eq\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html#tymethod.eq\" class=\"fn\">eq</a>(&amp;self, other: &amp;<a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>self</code> and <code>other</code> values to be equal, and is used\nby <code>==</code>.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.ne\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/nightly/src/core/cmp.rs.html#239\">source</a></span><a href=\"#method.ne\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html#method.ne\" class=\"fn\">ne</a>(&amp;self, other: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.reference.html\">&amp;Rhs</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>!=</code>. The default implementation is almost always\nsufficient, and should not be overridden without very good reason.</div></details></div></details>","PartialEq","lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#impl-Clone-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> for <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;<span class=\"where fmt-newline\">where\n    T::<a class=\"associatedtype\" href=\"lurk/hash_witness/trait.ContentAddressed.html#associatedtype.ScalarPtrRepr\" title=\"type lurk::hash_witness::ContentAddressed::ScalarPtrRepr\">ScalarPtrRepr</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a>,</span></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#method.clone\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html#tymethod.clone\" class=\"fn\">clone</a>(&amp;self) -&gt; <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;</h4></section></summary><div class='docblock'>Returns a copy of the value. <a href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html#tymethod.clone\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone_from\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/nightly/src/core/clone.rs.html#169\">source</a></span><a href=\"#method.clone_from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html#method.clone_from\" class=\"fn\">clone_from</a>(&amp;mut self, source: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.reference.html\">&amp;Self</a>)</h4></section></summary><div class='docblock'>Performs copy-assignment from <code>source</code>. <a href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html#method.clone_from\">Read more</a></div></details></div></details>","Clone","lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Debug-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#impl-Debug-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> for <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;<span class=\"where fmt-newline\">where\n    T::<a class=\"associatedtype\" href=\"lurk/hash_witness/trait.ContentAddressed.html#associatedtype.ScalarPtrRepr\" title=\"type lurk::hash_witness::ContentAddressed::ScalarPtrRepr\">ScalarPtrRepr</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a>,</span></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#342\">source</a><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html#tymethod.fmt\" class=\"fn\">fmt</a>(&amp;self, f: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/nightly/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;) -&gt; <a class=\"type\" href=\"https://doc.rust-lang.org/nightly/core/fmt/type.Result.html\" title=\"type core::fmt::Result\">Result</a></h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/nightly/core/fmt/trait.Debug.html#tymethod.fmt\">Read more</a></div></details></div></details>","Debug","lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CHashWitness%3CName,+T,+L,+F%3E%3E-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#350-360\">source</a><a href=\"#impl-From%3CHashWitness%3CName,+T,+L,+F%3E%3E-for-CircuitHashWitness%3CName,+T,+L,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;Name: <a class=\"trait\" href=\"lurk/hash_witness/trait.HashName.html\" title=\"trait lurk::hash_witness::HashName\">HashName</a>, T: <a class=\"trait\" href=\"lurk/hash_witness/trait.ContentAddressed.html\" title=\"trait lurk::hash_witness::ContentAddressed\">ContentAddressed</a>&lt;F&gt;, const L: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.usize.html\">usize</a>, F: <a class=\"trait\" href=\"lurk/field/trait.LurkField.html\" title=\"trait lurk::field::LurkField\">LurkField</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"lurk/hash_witness/struct.HashWitness.html\" title=\"struct lurk::hash_witness::HashWitness\">HashWitness</a>&lt;Name, T, L, F&gt;&gt; for <a class=\"struct\" href=\"lurk/hash_witness/struct.CircuitHashWitness.html\" title=\"struct lurk::hash_witness::CircuitHashWitness\">CircuitHashWitness</a>&lt;Name, T, L, F&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/lurk/hash_witness.rs.html#353-359\">source</a><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(hash_witness: <a class=\"struct\" href=\"lurk/hash_witness/struct.HashWitness.html\" title=\"struct lurk::hash_witness::HashWitness\">HashWitness</a>&lt;Name, T, L, F&gt;) -&gt; Self</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<HashWitness<Name, T, L, F>>","lurk::hash_witness::ConsCircuitWitness","lurk::hash_witness::ContCircuitWitness"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()