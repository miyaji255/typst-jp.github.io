//! Documentation provider for Typst.

mod contribs;
mod html;
mod link;
mod model;

pub use self::contribs::*;
pub use self::html::*;
pub use self::model::*;

use std::path::Path;

use comemo::Prehashed;
use ecow::{eco_format, EcoString};
use heck::ToTitleCase;
use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_yaml as yaml;
use typst::diag::{bail, StrResult};
use typst::foundations::{
    CastInfo, Category, Func, Module, ParamInfo, Repr, Scope, Smart, Type, Value,
    FOUNDATIONS,
};
use typst::introspection::INTROSPECTION;
use typst::layout::{Abs, Frame, Margin, PageElem, LAYOUT};
use typst::loading::DATA_LOADING;
use typst::math::MATH;
use typst::model::MODEL;
use typst::symbols::SYMBOLS;
use typst::text::{Font, FontBook, TEXT};
use typst::visualize::VISUALIZE;
use typst::Library;

static DOCS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../docs");
static FILE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../assets/files");
static FONT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../assets/fonts");

static GROUPS: Lazy<Vec<GroupData>> = Lazy::new(|| {
    let mut groups: Vec<GroupData> = yaml("reference/groups.yml");
    for group in &mut groups {
        if group.filter.is_empty() {
            group.filter = group
                .module()
                .scope()
                .iter()
                .filter(|(_, v)| matches!(v, Value::Func(_)))
                .map(|(k, _)| k.clone())
                .collect();
        }
    }
    groups
});

static LIBRARY: Lazy<Prehashed<Library>> = Lazy::new(|| {
    let mut lib = Library::build();
    lib.styles
        .set(PageElem::set_width(Smart::Custom(Abs::pt(240.0).into())));
    lib.styles.set(PageElem::set_height(Smart::Auto));
    lib.styles.set(PageElem::set_margin(Margin::splat(Some(Smart::Custom(
        Abs::pt(15.0).into(),
    )))));
    Prehashed::new(lib)
});

static FONTS: Lazy<(Prehashed<FontBook>, Vec<Font>)> = Lazy::new(|| {
    let fonts: Vec<_> = FONT_DIR
        .files()
        .flat_map(|file| Font::iter(file.contents().into()))
        .collect();
    let book = FontBook::from_fonts(&fonts);
    (Prehashed::new(book), fonts)
});

/// Build documentation pages.
pub fn provide(resolver: &dyn Resolver) -> Vec<PageModel> {
    vec![
        markdown_page(resolver, "/docs/", "overview.md").with_route("/docs/"),
        tutorial_pages(resolver),
        markdown_page(resolver, "/docs/", "chinese.md"),
        reference_pages(resolver),
        guide_pages(resolver),
        packages_page(resolver),
        markdown_page(resolver, "/docs/", "changelog.md"),
        markdown_page(resolver, "/docs/", "roadmap.md"),
        markdown_page(resolver, "/docs/", "community.md"),
        markdown_page(resolver, "/docs/", "glossary.md"),
    ]
}

/// Resolve consumer dependencies.
pub trait Resolver {
    /// Try to resolve a link that the system cannot resolve itself.
    fn link(&self, link: &str) -> Option<String>;

    /// Produce an URL for an image file.
    fn image(&self, filename: &str, data: &[u8]) -> String;

    /// Produce HTML for an example.
    fn example(&self, hash: u128, source: Option<Html>, frames: &[Frame]) -> Html;

    /// Determine the commits between two tags.
    fn commits(&self, from: &str, to: &str) -> Vec<Commit>;
}

/// Create a page from a markdown file.
#[track_caller]
fn markdown_page(
    resolver: &dyn Resolver,
    parent: &str,
    path: impl AsRef<Path>,
) -> PageModel {
    assert!(parent.starts_with('/') && parent.ends_with('/'));
    let md = DOCS_DIR.get_file(path).unwrap().contents_utf8().unwrap();
    let html = Html::markdown(resolver, md, Some(0));
    let title: EcoString = html.title().expect("chapter lacks a title").into();
    PageModel {
        route: eco_format!("{parent}{}/", urlify(&title)),
        title,
        description: html.description().unwrap(),
        part: None,
        outline: html.outline(),
        body: BodyModel::Html(html),
        children: vec![],
    }
}

/// Build the tutorial.
fn tutorial_pages(resolver: &dyn Resolver) -> PageModel {
    let mut page = markdown_page(resolver, "/docs/", "tutorial/welcome.md");
    page.children = DOCS_DIR
        .get_dir("tutorial")
        .unwrap()
        .files()
        .filter(|file| file.path() != Path::new("tutorial/welcome.md"))
        .map(|file| markdown_page(resolver, "/docs/tutorial/", file.path()))
        .collect();
    page
}

/// Build the reference.
fn reference_pages(resolver: &dyn Resolver) -> PageModel {
    let mut page = markdown_page(resolver, "/docs/", "reference/welcome.md");
    page.children = vec![
        markdown_page(resolver, "/docs/reference/", "reference/syntax.md")
            .with_part("Language"),
        markdown_page(resolver, "/docs/reference/", "reference/styling.md"),
        markdown_page(resolver, "/docs/reference/", "reference/scripting.md"),
        category_page(resolver, FOUNDATIONS).with_part("Library"),
        category_page(resolver, MODEL),
        category_page(resolver, TEXT),
        category_page(resolver, MATH),
        category_page(resolver, SYMBOLS),
        category_page(resolver, LAYOUT),
        category_page(resolver, VISUALIZE),
        category_page(resolver, INTROSPECTION),
        category_page(resolver, DATA_LOADING),
    ];
    page
}

/// Build the guides section.
fn guide_pages(resolver: &dyn Resolver) -> PageModel {
    let mut page = markdown_page(resolver, "/docs/", "guides/welcome.md");
    page.children = vec![
        markdown_page(resolver, "/docs/guides/", "guides/guide-for-latex-users.md"),
        markdown_page(resolver, "/docs/guides/", "guides/page-setup.md"),
    ];
    page
}

/// Build the packages section.
fn packages_page(resolver: &dyn Resolver) -> PageModel {
    let md = DOCS_DIR
        .get_file("reference/packages.md")
        .unwrap()
        .contents_utf8()
        .unwrap();
    PageModel {
        route: "/docs/packages/".into(),
        title: "第三方包".into(),
        description: "Typst 的第三方包".into(),
        part: None,
        outline: vec![],
        body: BodyModel::Packages(Html::markdown(resolver, md, Some(1))),
        children: vec![],
    }
}

/// Create a page for a category.
#[track_caller]
fn category_page(resolver: &dyn Resolver, category: Category) -> PageModel {
    let route = eco_format!("/docs/reference/{}/", category.name());
    let mut children = vec![];
    let mut items = vec![];
    let mut shorthands = None;
    let mut markup = vec![];
    let mut math = vec![];

    let (module, path): (&Module, &[&str]) = if category == MATH {
        (&LIBRARY.math, &["math"])
    } else {
        (&LIBRARY.global, &[])
    };

    // Add groups.
    for group in GROUPS.iter().filter(|g| g.category == category.name()).cloned() {
        if matches!(group.name.as_str(), "sym" | "emoji") {
            let subpage = symbols_page(resolver, &route, &group);
            let BodyModel::Symbols(model) = &subpage.body else { continue };
            let list = &model.list;
            markup.extend(
                list.iter()
                    .filter(|symbol| symbol.markup_shorthand.is_some())
                    .cloned(),
            );
            math.extend(
                list.iter().filter(|symbol| symbol.math_shorthand.is_some()).cloned(),
            );

            items.push(CategoryItem {
                name: group.name.clone(),
                route: subpage.route.clone(),
                oneliner: oneliner(category.docs()).into(),
                code: true,
            });
            children.push(subpage);
            continue;
        }

        let (child, item) = group_page(resolver, &route, &group);
        children.push(child);
        items.push(item);
    }

    // Add symbol pages. These are ordered manually.
    if category == SYMBOLS {
        shorthands = Some(ShorthandsModel { markup, math });
    }

    // Add functions.
    let scope = module.scope();
    for (name, value) in scope.iter() {
        if scope.get_category(name) != Some(category) {
            continue;
        }

        if category == MATH {
            // Skip grouped functions.
            if GROUPS.iter().flat_map(|group| &group.filter).any(|f| f == name) {
                continue;
            }

            // Already documented in the text category.
            if name == "text" {
                continue;
            }
        }

        match value {
            Value::Func(func) => {
                let name = func.name().unwrap();

                let subpage = func_page(resolver, &route, func, path);
                items.push(CategoryItem {
                    name: name.into(),
                    route: subpage.route.clone(),
                    oneliner: oneliner(func.docs().unwrap_or_default()).into(),
                    code: true,
                });
                children.push(subpage);
            }
            Value::Type(ty) => {
                let subpage = type_page(resolver, &route, ty);
                items.push(CategoryItem {
                    name: ty.short_name().into(),
                    route: subpage.route.clone(),
                    oneliner: oneliner(ty.docs()).into(),
                    code: true,
                });
                children.push(subpage);
            }
            _ => {}
        }
    }

    if category != SYMBOLS {
        children.sort_by_cached_key(|child| child.title.clone());
        items.sort_by_cached_key(|item| item.name.clone());
    }

    // let name = category.title();
    let title_name = category.title();
    let name = match title_name {
        "text" => "文本",
        "math" => "数学",
        "layout" => "布局",
        "visualize" => "可视化",
        "meta" => "元信息",
        "symbols" => "符号",
        "foundations" => "基础",
        "calculate" => "计算",
        "construct" => "构造",
        "data-loading" => "数据加载",
        _ => &title_name,
    };
    let details = Html::markdown(resolver, category.docs(), Some(1));
    let mut outline = vec![OutlineItem::from_name("Summary")];
    outline.extend(details.outline());
    outline.push(OutlineItem::from_name("Definitions"));
    if shorthands.is_some() {
        outline.push(OutlineItem::from_name("Shorthands"));
    }

    PageModel {
        route,
        title: name.into(),
        description: eco_format!("Typst 中与 {name} 有关联的函数族的文档"),
        part: None,
        outline,
        body: BodyModel::Category(CategoryModel {
            name: category.name(),
            title: category.title(),
            details,
            items,
            shorthands,
        }),
        children,
    }
}

/// Create a page for a function.
fn func_page(
    resolver: &dyn Resolver,
    parent: &str,
    func: &Func,
    path: &[&str],
) -> PageModel {
    let model = func_model(resolver, func, path, false);
    let name = func.name().unwrap();
    PageModel {
        route: eco_format!("{parent}{}/", urlify(name)),
        title: func.title().unwrap().into(),
        description: eco_format!("`{name}` 函数的文档"),
        part: None,
        outline: func_outline(&model, ""),
        body: BodyModel::Func(model),
        children: vec![],
    }
}

/// Produce a function's model.
fn func_model(
    resolver: &dyn Resolver,
    func: &Func,
    path: &[&str],
    nested: bool,
) -> FuncModel {
    let name = func.name().unwrap();
    let scope = func.scope().unwrap();
    let docs = func.docs().unwrap();

    let mut self_ = false;
    let mut params = func.params().unwrap();
    if params.first().map_or(false, |first| first.name == "self") {
        self_ = true;
        params = &params[1..];
    }

    let mut returns = vec![];
    casts(resolver, &mut returns, &mut vec![], func.returns().unwrap());
    returns.sort_by_key(|ty| type_index(ty));
    if returns == ["none"] {
        returns.clear();
    }

    let nesting = if nested { None } else { Some(1) };
    let (details, example) =
        if nested { split_details_and_example(docs) } else { (docs, None) };

    FuncModel {
        path: path.iter().copied().map(Into::into).collect(),
        name: name.into(),
        title: func.title().unwrap(),
        keywords: func.keywords(),
        oneliner: oneliner(details),
        element: func.element().is_some(),
        details: Html::markdown(resolver, details, nesting),
        example: example.map(|md| Html::markdown(resolver, md, None)),
        self_,
        params: params.iter().map(|param| param_model(resolver, param)).collect(),
        returns,
        scope: scope_models(resolver, name, scope),
    }
}

/// Produce a parameter's model.
fn param_model(resolver: &dyn Resolver, info: &ParamInfo) -> ParamModel {
    let (details, example) = split_details_and_example(info.docs);

    let mut types = vec![];
    let mut strings = vec![];
    casts(resolver, &mut types, &mut strings, &info.input);
    if !strings.is_empty() && !types.contains(&"str") {
        types.push("str");
    }
    types.sort_by_key(|ty| type_index(ty));

    ParamModel {
        name: info.name,
        details: Html::markdown(resolver, details, None),
        example: example.map(|md| Html::markdown(resolver, md, None)),
        types,
        strings,
        default: info.default.map(|default| {
            let node = typst::syntax::parse_code(&default().repr());
            Html::new(typst::syntax::highlight_html(&node))
        }),
        positional: info.positional,
        named: info.named,
        required: info.required,
        variadic: info.variadic,
        settable: info.settable,
    }
}

/// Split up documentation into details and an example.
fn split_details_and_example(docs: &str) -> (&str, Option<&str>) {
    let mut details = docs;
    let mut example = None;
    if let Some(mut i) = docs.find("```") {
        while docs[..i].ends_with('`') {
            i -= 1;
        }
        details = &docs[..i];
        example = Some(&docs[i..]);
    }
    (details, example)
}

/// Process cast information into types and strings.
fn casts(
    resolver: &dyn Resolver,
    types: &mut Vec<&'static str>,
    strings: &mut Vec<StrParam>,
    info: &CastInfo,
) {
    match info {
        CastInfo::Any => types.push("any"),
        CastInfo::Value(Value::Str(string), docs) => strings.push(StrParam {
            string: string.clone().into(),
            details: Html::markdown(resolver, docs, None),
        }),
        CastInfo::Value(..) => {}
        CastInfo::Type(ty) => types.push(ty.short_name()),
        CastInfo::Union(options) => {
            for option in options {
                casts(resolver, types, strings, option);
            }
        }
    }
}

/// Produce models for a function's scope.
fn scope_models(resolver: &dyn Resolver, name: &str, scope: &Scope) -> Vec<FuncModel> {
    scope
        .iter()
        .filter_map(|(_, value)| {
            let Value::Func(func) = value else { return None };
            Some(func_model(resolver, func, &[name], true))
        })
        .collect()
}

/// Produce an outline for a function page.
fn func_outline(model: &FuncModel, id_base: &str) -> Vec<OutlineItem> {
    let mut outline = vec![];

    if id_base.is_empty() {
        outline.push(OutlineItem::from_name("Summary"));
        outline.extend(model.details.outline());

        if !model.params.is_empty() {
            outline.push(OutlineItem {
                id: "parameters".into(),
                name: "Parameters".into(),
                children: model
                    .params
                    .iter()
                    .map(|param| OutlineItem {
                        id: eco_format!("parameters-{}", urlify(param.name)),
                        name: param.name.into(),
                        children: vec![],
                    })
                    .collect(),
            });
        }

        outline.extend(scope_outline(&model.scope));
    } else {
        outline.extend(model.params.iter().map(|param| OutlineItem {
            id: eco_format!("{id_base}-{}", urlify(param.name)),
            name: param.name.into(),
            children: vec![],
        }));
    }

    outline
}

/// Produce an outline for a function scope.
fn scope_outline(scope: &[FuncModel]) -> Option<OutlineItem> {
    if scope.is_empty() {
        return None;
    }

    Some(OutlineItem {
        id: "definitions".into(),
        name: "Definitions".into(),
        children: scope
            .iter()
            .map(|func| {
                let id = urlify(&eco_format!("definitions-{}", func.name));
                let children = func_outline(func, &id);
                OutlineItem { id, name: func.title.into(), children }
            })
            .collect(),
    })
}

/// Create a page for a group of functions.
fn group_page(
    resolver: &dyn Resolver,
    parent: &str,
    group: &GroupData,
) -> (PageModel, CategoryItem) {
    let mut functions = vec![];
    let mut outline = vec![OutlineItem::from_name("Summary")];

    let path: Vec<_> = group.path.iter().map(|s| s.as_str()).collect();
    let details = Html::markdown(resolver, &group.details, Some(1));
    outline.extend(details.outline());

    let mut outline_items = vec![];
    for name in &group.filter {
        let value = group.module().scope().get(name).unwrap();
        let Value::Func(func) = value else { panic!("not a function") };
        let func = func_model(resolver, func, &path, true);
        let id_base = urlify(&eco_format!("functions-{}", func.name));
        let children = func_outline(&func, &id_base);
        outline_items.push(OutlineItem {
            id: id_base,
            name: func.title.into(),
            children,
        });
        functions.push(func);
    }

    outline.push(OutlineItem {
        id: "functions".into(),
        name: "Functions".into(),
        children: outline_items,
    });

    let model = PageModel {
        route: eco_format!("{parent}{}", group.name),
        title: group.title.clone(),
        description: eco_format!("{} 函数族的文档.", group.name),
        part: None,
        outline,
        body: BodyModel::Group(GroupModel {
            name: group.name.clone(),
            title: group.title.clone(),
            details,
            functions,
        }),
        children: vec![],
    };

    let item = CategoryItem {
        name: group.name.clone(),
        route: model.route.clone(),
        oneliner: oneliner(&group.details).into(),
        code: false,
    };

    (model, item)
}

/// Create a page for a type.
fn type_page(resolver: &dyn Resolver, parent: &str, ty: &Type) -> PageModel {
    let model = type_model(resolver, ty);
    PageModel {
        route: eco_format!("{parent}{}/", urlify(ty.short_name())),
        title: ty.title().into(),
        description: eco_format!("{} 类型的文档", ty.title()),
        part: None,
        outline: type_outline(&model),
        body: BodyModel::Type(model),
        children: vec![],
    }
}

/// Produce a type's model.
fn type_model(resolver: &dyn Resolver, ty: &Type) -> TypeModel {
    TypeModel {
        name: ty.short_name(),
        title: ty.title(),
        keywords: ty.keywords(),
        oneliner: oneliner(ty.docs()),
        details: Html::markdown(resolver, ty.docs(), Some(1)),
        constructor: ty
            .constructor()
            .ok()
            .map(|func| func_model(resolver, &func, &[], true)),
        scope: scope_models(resolver, ty.short_name(), ty.scope()),
    }
}

/// Produce an outline for a type page.
fn type_outline(model: &TypeModel) -> Vec<OutlineItem> {
    let mut outline = vec![OutlineItem::from_name("Summary")];
    outline.extend(model.details.outline());

    if let Some(func) = &model.constructor {
        outline.push(OutlineItem {
            id: "constructor".into(),
            name: "Constructor".into(),
            children: func_outline(func, "constructor"),
        });
    }

    outline.extend(scope_outline(&model.scope));
    outline
}

/// Create a page for symbols.
fn symbols_page(resolver: &dyn Resolver, parent: &str, group: &GroupData) -> PageModel {
    let model = symbols_model(resolver, group);
    PageModel {
        route: eco_format!("{parent}{}/", group.name),
        title: group.title.clone(),
        description: eco_format!("`{}` 模块的文档", group.name),
        part: None,
        outline: vec![],
        body: BodyModel::Symbols(model),
        children: vec![],
    }
}

/// Produce a symbol list's model.
fn symbols_model(resolver: &dyn Resolver, group: &GroupData) -> SymbolsModel {
    let mut list = vec![];
    for (name, value) in group.module().scope().iter() {
        let Value::Symbol(symbol) = value else { continue };
        let complete = |variant: &str| {
            if variant.is_empty() {
                name.clone()
            } else {
                eco_format!("{}.{}", name, variant)
            }
        };

        for (variant, c) in symbol.variants() {
            let shorthand = |list: &[(&'static str, char)]| {
                list.iter().copied().find(|&(_, x)| x == c).map(|(s, _)| s)
            };

            list.push(SymbolModel {
                name: complete(variant),
                markup_shorthand: shorthand(typst::syntax::ast::Shorthand::MARKUP_LIST),
                math_shorthand: shorthand(typst::syntax::ast::Shorthand::MATH_LIST),
                codepoint: c as u32,
                accent: typst::symbols::Symbol::combining_accent(c).is_some(),
                unicode_name: unicode_names2::name(c)
                    .map(|s| s.to_string().to_title_case().into()),
                alternates: symbol
                    .variants()
                    .filter(|(other, _)| other != &variant)
                    .map(|(other, _)| complete(other))
                    .collect(),
            });
        }
    }

    SymbolsModel {
        name: group.name.clone(),
        title: group.title.clone(),
        details: Html::markdown(resolver, &group.details, Some(1)),
        list,
    }
}

/// Extract a module from another module.
#[track_caller]
fn get_module<'a>(parent: &'a Module, name: &str) -> StrResult<&'a Module> {
    match parent.scope().get(name) {
        Some(Value::Module(module)) => Ok(module),
        _ => bail!("module doesn't contain module `{name}`"),
    }
}

/// Load YAML from a path.
#[track_caller]
fn yaml<T: DeserializeOwned>(path: &str) -> T {
    let file = DOCS_DIR.get_file(path).unwrap();
    yaml::from_slice(file.contents()).unwrap()
}

/// Turn a title into an URL fragment.
pub fn urlify(title: &str) -> EcoString {
    match title {
        "教程" => "tutorial".into(),
        "使用 Typst 写作" => "writing-in-typst".into(),
        "格式" => "formatting".into(),
        "高级样式" => "advanced-styling".into(),
        "制作模板" => "making-a-template".into(),
        "中文用户指南" => "chinese".into(),
        "参考" => "reference".into(),
        "语法" => "syntax".into(),
        "样式" => "styling".into(),
        "脚本" => "scripting".into(),
        "指南" => "guides".into(),
        "LaTeX 用户指南" => "guide-for-latex-users".into(),
        "页面设置指南" => "page-setup".into(),
        "更新日志" => "changelog".into(),
        "路线图" => "roadmap".into(),
        "社区" => "community".into(),
        "术语表" => "glossary".into(),
        _ => title
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .map(|c| match c {
                'a'..='z' | '0'..='9' => c,
                _ => '-',
            })
            .collect(),
    }
}

/// Extract the first line of documentation.
fn oneliner(docs: &str) -> &str {
    docs.lines().next().unwrap_or_default()
}

/// The order of types in the documentation.
fn type_index(ty: &str) -> usize {
    TYPE_ORDER.iter().position(|&v| v == ty).unwrap_or(usize::MAX)
}

const TYPE_ORDER: &[&str] = &[
    "any",
    "none",
    "auto",
    "bool",
    "int",
    "float",
    "length",
    "angle",
    "ratio",
    "relative",
    "fraction",
    "color",
    "gradient",
    "datetime",
    "duration",
    "str",
    "bytes",
    "regex",
    "label",
    "content",
    "array",
    "dict",
    "func",
    "args",
    "selector",
    "location",
    "direction",
    "alignment",
    "alignment2d",
    "stroke",
];

/// Data about a collection of functions.
#[derive(Debug, Clone, Deserialize)]
struct GroupData {
    name: EcoString,
    title: EcoString,
    category: EcoString,
    #[serde(default)]
    path: Vec<EcoString>,
    #[serde(default)]
    filter: Vec<EcoString>,
    details: EcoString,
}

impl GroupData {
    fn module(&self) -> &'static Module {
        let mut focus = &LIBRARY.global;
        for path in &self.path {
            focus = get_module(focus, path).unwrap();
        }
        focus
    }
}

#[cfg(test)]
mod tests {
    use md5;
    use std::io::Write;
    use typst::visualize::Color;

    use super::*;

    #[test]
    fn test_docs() {
        // remove all files in ../../assets/docs
        let _ = std::fs::remove_dir_all("../../assets/docs");
        // copy all files from ../../assets/files to ../../assets/docs
        std::fs::create_dir("../../assets/docs").unwrap();
        for entry in std::fs::read_dir("../../assets/files").unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = String::from(path.file_name().unwrap().to_str().unwrap());
            std::fs::copy(path, format!("../../assets/docs/{}", name)).unwrap();
        }
        // convert all pages to html and generate example images to ../../assets/docs
        let pages = provide(&TestResolver);
        // convert pages to JSON and save to ../../assets/docs.json
        let json = serde_json::to_string_pretty(&pages).unwrap();
        let mut file = std::fs::File::create("../../assets/docs.json").unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }

    struct TestResolver;

    impl Resolver for TestResolver {
        fn link(&self, _: &str) -> Option<String> {
            None
        }

        fn example(&self, _: u128, source: Option<Html>, frames: &[Frame]) -> Html {
            // convert frames to a png
            let ppi = 2.0;
            // the first frame is the main frame
            let pixmap = typst_render::render(frames.first().unwrap(), ppi, Color::WHITE);
            // Get a random filename by md5
            match source {
                Some(source) => {
                    let filename = format!("{:x}.png", md5::compute(source.as_str()));
                    let path = Path::new("../../assets/docs").join(filename.clone());
                    let _ = pixmap.save_png(path).map_err(|_| "failed to write PNG file");
                    Html::new(format!(
                        r#"<div class="previewed-code"><pre>{}</pre><div class="preview"><img src="/assets/docs/{}" alt="Preview" width="480" height="190"/></div></div>"#,
                        source.as_str(),
                        filename
                    ))
                }
                _ => Html::new(String::new()),
            }
        }

        fn image(&self, filename: &str, _: &[u8]) -> String {
            // return /assets/docs/<filename>
            format!("/assets/docs/{}", filename)
        }

        fn commits(&self, _: &str, _: &str) -> Vec<Commit> {
            vec![]
        }
    }
}
