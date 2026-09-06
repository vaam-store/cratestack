//! `--rtk`'s (issue #906) model endpoint-key naming scheme — deliberately
//! NOT `crate::naming::model_fn_names` (the `--swr` scheme), even though
//! both derive five per-verb identifiers from a model name.
//!
//! # Why a new scheme, not the existing one
//!
//! `model_fn_names` pluralizes (`list{pluralize(Model)}`) so a model named
//! `Post` and one named `Posts` both derive `listPosts` — an accepted,
//! documented gap there (`crate::swr::collisions`' own doc: "two models
//! whose derived function names collide with each other is a separate
//! defect this check does not own") because `--swr` splits model and
//! procedure bindings across *files* that a barrel `export *`s, so the
//! worse failure mode (two `export *`-visible bindings of the same name)
//! is already guarded against for the procedure side, and a same-scheme
//! model-vs-model collision is rarer and undetected either way.
//!
//! `--rtk` cannot inherit that gap: RTK Query's endpoint map is ONE object
//! literal (this ticket's own stated risk — see `crate::rtk`'s module
//! doc), so a `Post`/`Posts` collision here would be a same-object
//! duplicate key, not a cross-file one. Using the RAW model name instead
//! of a pluralized form (`list{{ model.name }}`, not
//! `list{{ pluralize(model.name) }}`) makes a model-vs-model collision
//! structurally impossible: the parser already guarantees model names are
//! unique verbatim across the schema, and `list{A}` == `list{B}` implies
//! `A == B`. Only a procedure-vs-model collision remains reachable, which
//! `crate::rtk::collisions` guards the same way `crate::tanstack_collisions`
//! guards its own (structurally distinct) hook-name scheme.
pub(crate) struct RtkEndpointNames {
    pub(crate) list: String,
    pub(crate) get: String,
    pub(crate) create: String,
    pub(crate) update: String,
    pub(crate) delete: String,
}

pub(crate) fn rtk_endpoint_names(model_name: &str) -> RtkEndpointNames {
    RtkEndpointNames {
        list: crate::naming::to_camel_case(&format!("list_{model_name}")),
        get: crate::naming::to_camel_case(&format!("get_{model_name}")),
        create: crate::naming::to_camel_case(&format!("create_{model_name}")),
        update: crate::naming::to_camel_case(&format!("update_{model_name}")),
        delete: crate::naming::to_camel_case(&format!("delete_{model_name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_model_names_can_never_collide_even_when_one_pluralizes_to_the_other() {
        // `Post`/`Posts` is the exact pair `--swr`'s `model_fn_names`
        // cannot tell apart (both pluralize to `Posts`) — this scheme
        // must, since it never pluralizes at all.
        let post = rtk_endpoint_names("Post");
        let posts = rtk_endpoint_names("Posts");
        assert_ne!(post.list, posts.list);
        assert_ne!(post.get, posts.get);
        assert_ne!(post.create, posts.create);
        assert_ne!(post.update, posts.update);
        assert_ne!(post.delete, posts.delete);
    }

    #[test]
    fn derives_the_expected_camel_case_identifiers() {
        let widget = rtk_endpoint_names("Widget");
        assert_eq!(widget.list, "listWidget");
        assert_eq!(widget.get, "getWidget");
        assert_eq!(widget.create, "createWidget");
        assert_eq!(widget.update, "updateWidget");
        assert_eq!(widget.delete, "deleteWidget");
    }
}
