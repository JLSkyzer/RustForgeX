//! Index des classes chargees et selection de la methode executee (JVMS §5.4.6).
//!
//! Cahier des charges : PARTIE 5.7 (resolution par hierarchie de classes chargees),
//! FM-15, R-341. Decision : ADR-050. Maturite : `STABLE`.
//!
//! # Prudence
//!
//! L'index ne connait que les classes qu'on lui a donnees. Toute recherche qui croise
//! une classe absente sans avoir trouve sa reponse rend [`Selection::Unknown`] : la
//! classe absente aurait pu fournir la methode, et deviner serait affirmer un appel
//! « resolu » qui ne l'est peut-etre pas.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use rfx_bytecode::ClassFile;

/// Methode designee par sa classe, son nom et son descripteur.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MethodRef {
    /// Nom interne de la classe (`net/minecraft/world/entity/Mob`).
    pub owner: String,
    /// Nom.
    pub name: String,
    /// Descripteur (`()V`).
    pub descriptor: String,
}

impl MethodRef {
    /// Construit une reference.
    #[must_use]
    pub fn new(owner: &str, name: &str, descriptor: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        }
    }
}

impl core::fmt::Display for MethodRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}{}", self.owner, self.name, self.descriptor)
    }
}

/// Issue de la recherche de la methode executee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// La methode executee.
    Found(MethodRef),
    /// Aucune methode concrete : l'appel leverait `AbstractMethodError`.
    Missing,
    /// Une classe absente de l'index aurait pu decider : rien n'est affirme.
    Unknown,
    /// Plusieurs methodes par defaut candidates : rien n'est affirme.
    Ambiguous,
}

/// Index des classes chargees.
#[derive(Debug, Default)]
pub struct ClassIndex {
    classes: HashMap<String, ClassFile>,
    /// Sous-types directs : classes qui etendent, interfaces et classes qui implementent.
    subtypes: HashMap<String, Vec<String>>,
}

impl ClassIndex {
    /// Index vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Index de ces classes.
    #[must_use]
    pub fn from_classes(classes: impl IntoIterator<Item = ClassFile>) -> Self {
        let mut index = Self::new();
        for class in classes {
            index.insert(class);
        }
        index
    }

    /// Ajoute (ou remplace) une classe.
    pub fn insert(&mut self, class: ClassFile) {
        if let Some(previous) = self.classes.get(&class.name) {
            let parents: Vec<String> = parents(previous).map(str::to_owned).collect();
            for parent in parents {
                if let Some(children) = self.subtypes.get_mut(&parent) {
                    children.retain(|child| child != &class.name);
                }
            }
        }
        for parent in parents(&class) {
            self.subtypes
                .entry(parent.to_owned())
                .or_default()
                .push(class.name.clone());
        }
        self.classes.insert(class.name.clone(), class);
    }

    /// Classe par son nom interne.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ClassFile> {
        self.classes.get(name)
    }

    /// Nombre de classes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.classes.len()
    }

    /// `true` si l'index est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Classes concretes chargees qui sont `name` ou en descendent, dans un ordre
    /// stable. `None` si `name` est absente de l'index.
    #[must_use]
    pub fn concrete_subtypes(&self, name: &str) -> Option<Vec<&ClassFile>> {
        self.classes.get(name)?;
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([name]);
        let mut found = BTreeSet::new();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current) {
                continue;
            }
            if let Some(class) = self.classes.get(current) {
                if !class.is_interface() && !class.is_abstract() {
                    found.insert(current);
                }
            }
            if let Some(children) = self.subtypes.get(current) {
                queue.extend(children.iter().map(String::as_str));
            }
        }
        Some(
            found
                .into_iter()
                .filter_map(|n| self.classes.get(n))
                .collect(),
        )
    }

    /// Methode executee quand `name descriptor` est appelee sur un objet de classe
    /// `receiver` (JVMS §5.4.6) : d'abord la classe et ses super-classes, puis les
    /// methodes par defaut des interfaces.
    #[must_use]
    pub fn select(&self, receiver: &str, name: &str, descriptor: &str) -> Selection {
        // 1. La classe et ses super-classes.
        let mut chain = Vec::new();
        let mut current = Some(receiver);
        while let Some(class_name) = current {
            let Some(class) = self.classes.get(class_name) else {
                // `java/lang/Object` est rarement indexee, mais ses methodes sont fixees
                // par le JDK : elle decide sans qu'on ait besoin de son bytecode.
                if class_name == OBJECT {
                    if OBJECT_METHODS.contains(&(name, descriptor)) {
                        return Selection::Found(MethodRef::new(OBJECT, name, descriptor));
                    }
                    break;
                }
                return Selection::Unknown;
            };
            if let Some(method) = class.method(name, descriptor) {
                // Une methode statique ou privee d'une super-classe n'est pas une
                // redefinition : la recherche continue au-dessus.
                let overrides =
                    !method.is_static() && (!method.is_private() || class_name == receiver);
                if overrides {
                    return if method.is_abstract() {
                        Selection::Missing
                    } else {
                        Selection::Found(MethodRef::new(class_name, name, descriptor))
                    };
                }
            }
            chain.push(class);
            current = class.super_name.as_deref();
        }

        // 2. Les methodes par defaut des interfaces, transitivement.
        let mut defaults = BTreeSet::new();
        let mut unknown = false;
        let mut seen = HashSet::new();
        let mut queue: VecDeque<&str> = chain
            .iter()
            .flat_map(|class| class.interfaces.iter().map(String::as_str))
            .collect();
        while let Some(interface) = queue.pop_front() {
            if !seen.insert(interface) {
                continue;
            }
            let Some(class) = self.classes.get(interface) else {
                unknown = true;
                continue;
            };
            if let Some(method) = class.method(name, descriptor) {
                if !method.is_abstract() && !method.is_static() && !method.is_private() {
                    defaults.insert(MethodRef::new(interface, name, descriptor));
                }
            }
            queue.extend(class.interfaces.iter().map(String::as_str));
        }
        if unknown {
            return Selection::Unknown;
        }
        match defaults.len() {
            0 => Selection::Missing,
            1 => defaults
                .into_iter()
                .next()
                .map_or(Selection::Missing, Selection::Found),
            // Le choix de la plus specifique (JVMS §5.4.3.3) n'est pas fait : deux
            // candidates, c'est un appel non resolu.
            _ => Selection::Ambiguous,
        }
    }
}

/// Racine de toute hierarchie de classes.
const OBJECT: &str = "java/lang/Object";

/// Methodes d'instance de `java/lang/Object` (JDK 17), les seules qu'elle puisse fournir
/// a une selection.
const OBJECT_METHODS: [(&str, &str); 11] = [
    ("equals", "(Ljava/lang/Object;)Z"),
    ("hashCode", "()I"),
    ("toString", "()Ljava/lang/String;"),
    ("getClass", "()Ljava/lang/Class;"),
    ("clone", "()Ljava/lang/Object;"),
    ("finalize", "()V"),
    ("notify", "()V"),
    ("notifyAll", "()V"),
    ("wait", "()V"),
    ("wait", "(J)V"),
    ("wait", "(JI)V"),
];

/// Super-classe et interfaces directes.
fn parents(class: &ClassFile) -> impl Iterator<Item = &str> {
    class
        .super_name
        .as_deref()
        .into_iter()
        .chain(class.interfaces.iter().map(String::as_str))
}
