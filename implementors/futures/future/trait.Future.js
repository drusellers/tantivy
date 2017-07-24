(function() {var implementors = {};
implementors["futures"] = [];
implementors["futures_cpupool"] = ["impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + 'static, E:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + 'static&gt; <a class=\"trait\" href=\"futures/future/trait.Future.html\" title=\"trait futures::future::Future\">Future</a> for <a class=\"struct\" href=\"futures_cpupool/struct.CpuFuture.html\" title=\"struct futures_cpupool::CpuFuture\">CpuFuture</a>&lt;T, E&gt;",];
implementors["tantivy"] = ["impl&lt;T, E&gt; <a class=\"trait\" href=\"futures/future/trait.Future.html\" title=\"trait futures::future::Future\">Future</a> for <a class=\"struct\" href=\"futures_cpupool/struct.CpuFuture.html\" title=\"struct futures_cpupool::CpuFuture\">CpuFuture</a>&lt;T, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;E: 'static + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: 'static + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,&nbsp;</span>",];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        
})()
