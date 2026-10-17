//! Index des classes et selection de la methode executee (JVMS §5.4.6).
//!
//! Cahier des charges : PARTIE 5.7 (resolution par hierarchie de classes chargees),
//! FM-15, R-341. Decisions : ADR-050, ADR-051. Maturite : `STABLE`.
//!
//! # Deux couches
//!
//! - La **hierarchie** de l'installation : pour chaque classe, sa super-classe et ses
//!   drapeaux ([`Skeleton`]), sans son bytecode. Elle suffit a enumerer les sous-classes
//!   d'une classe ; sans elle, l'enumeration serait incomplete et un appel pourrait etre
//!   declare monomorphe a tort.
//! - Le **bytecode** des classes deja fournies. Celui d'une classe connue de la
//!   hierarchie mais pas encore fournie est demande ([`ClassIndex::take_requests`]) ;
//!   l'analyse rend alors [`Selection::Pending`], et celui qui la pilote la relance une
//!   fois les classes fournies.
//!
//! # Prudence
//!
//! Toute recherche qui croise une classe inconnue de la hierarchie, ou declaree absente
//! (une cible de mixin, dont le bytecode final nous echappe), sans avoir trouve sa
//! reponse rend [`Selection::Unknown`] : cette classe aurait pu decider, et deviner
//! serait affirmer un appel « resolu » qui ne l'est peut-etre pas.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use rfx_bytecode::{access, ClassFile};

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

/// Ce que la hierarchie de l'installation dit d'une classe, sans son bytecode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    /// Super-classe ; absente pour `java/lang/Object` et les classes inconnues.
    pub super_name: Option<String>,
    /// Drapeaux d'acces de la classe.
    pub access: u16,
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
    /// Une classe connue de la hierarchie manque encore ; elle est demandee.
    Pending,
    /// Plusieurs methodes par defaut candidates : rien n'est affirme.
    Ambiguous,
}

/// Index des classes.
#[derive(Debug, Default)]
pub struct ClassIndex {
    classes: HashMap<String, ClassFile>,
    declared: HashMap<String, Skeleton>,
    absent: HashSet<String>,
    /// Sous-types directs : classes qui etendent, interfaces et classes qui implementent.
    subtypes: HashMap<String, BTreeSet<String>>,
    requests: RefCell<BTreeSet<String>>,
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

    /// Declare une classe de la hierarchie, sans son bytecode.
    pub fn declare(&mut self, name: &str, skeleton: Skeleton) {
        if let Some(parent) = &skeleton.super_name {
            self.subtypes
                .entry(parent.clone())
                .or_default()
                .insert(name.to_owned());
        }
        self.declared.insert(name.to_owned(), skeleton);
    }

    /// Declare qu'une classe ne sera pas fournie : son bytecode final est inconnu (cible
    /// de mixin) ou illisible.
    pub fn declare_absent(&mut self, name: &str) {
        self.absent.insert(name.to_owned());
        self.requests.borrow_mut().remove(name);
    }

    /// Ajoute (ou remplace) une classe et son bytecode.
    pub fn insert(&mut self, class: ClassFile) {
        if let Some(previous) = self.classes.get(&class.name) {
            let parents: Vec<String> = parents(previous).map(str::to_owned).collect();
            for parent in parents {
                if let Some(children) = self.subtypes.get_mut(&parent) {
                    children.remove(&class.name);
                }
            }
            // La hierarchie declaree, elle, reste vraie.
            if let Some(parent) = self
                .declared
                .get(&class.name)
                .and_then(|s| s.super_name.clone())
            {
                self.subtypes
                    .entry(parent)
                    .or_default()
                    .insert(class.name.clone());
            }
        }
        for parent in parents(&class) {
            self.subtypes
                .entry(parent.to_owned())
                .or_default()
                .insert(class.name.clone());
        }
        self.requests.borrow_mut().remove(&class.name);
        self.classes.insert(class.name.clone(), class);
    }

    /// Retire tout le bytecode, et garde la hierarchie declaree : pour borner la memoire.
    pub fn clear_bytecode(&mut self) {
        self.classes.clear();
        self.subtypes.clear();
        let declared: Vec<(String, Option<String>)> = self
            .declared
            .iter()
            .map(|(name, skeleton)| (name.clone(), skeleton.super_name.clone()))
            .collect();
        for (name, parent) in declared {
            if let Some(parent) = parent {
                self.subtypes.entry(parent).or_default().insert(name);
            }
        }
    }

    /// Classe deja fournie, sans rien demander.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ClassFile> {
        self.classes.get(name)
    }

    /// Classe deja fournie ; a defaut, la demande si la hierarchie la connait.
    #[must_use]
    pub fn class(&self, name: &str) -> Option<&ClassFile> {
        let class = self.classes.get(name);
        if class.is_none() {
            self.request(name);
        }
        class
    }

    /// `true` si la classe peut encore etre fournie : connue de la hierarchie, ni
    /// fournie, ni declaree absente.
    #[must_use]
    pub fn is_fetchable(&self, name: &str) -> bool {
        self.declared.contains_key(name)
            && !self.absent.contains(name)
            && !self.classes.contains_key(name)
    }

    /// Demande le bytecode d'une classe, si elle peut etre fournie.
    pub fn request(&self, name: &str) {
        if self.is_fetchable(name) {
            self.requests.borrow_mut().insert(name.to_owned());
        }
    }

    /// Classes demandees depuis le dernier appel, dans un ordre stable.
    #[must_use]
    pub fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut *self.requests.borrow_mut())
            .into_iter()
            .collect()
    }

    /// Classes dont le bytecode est present.
    #[must_use]
    pub fn len(&self) -> usize {
        self.classes.len()
    }

    /// `true` si aucun bytecode n'est present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Classes declarees dans la hierarchie.
    #[must_use]
    pub fn declared_len(&self) -> usize {
        self.declared.len()
    }

    /// Super-classe d'une classe : de son bytecode, a defaut de la hierarchie. `None` si
    /// la classe est inconnue ; `Some(None)` si elle n'a pas de super-classe.
    #[must_use]
    pub fn superclass(&self, name: &str) -> Option<Option<&str>> {
        match self.classes.get(name) {
            Some(class) => Some(class.super_name.as_deref()),
            None => self
                .declared
                .get(name)
                .map(|skeleton| skeleton.super_name.as_deref()),
        }
    }

    /// Drapeaux d'une classe : de son bytecode, a defaut de la hierarchie.
    fn access_of(&self, name: &str) -> Option<u16> {
        self.classes
            .get(name)
            .map(|class| class.access)
            .or_else(|| self.declared.get(name).map(|skeleton| skeleton.access))
    }

    /// Classes concretes qui sont `name` ou en descendent, d'apres le bytecode fourni et
    /// la hierarchie declaree, triees par nom. `None` si `name` est inconnue.
    ///
    /// Les noms sont empruntes a l'index, jamais copies : sous `java/lang/Object`, le
    /// sous-arbre est l'installation entiere, et le copier nom par nom dominait le cout
    /// d'un graphe (ADR-052).
    #[must_use]
    pub fn concrete_subtypes(&self, name: &str) -> Option<Vec<&str>> {
        // Le nom tel que l'index le detient : le resultat n'emprunte qu'a l'index.
        let name = self
            .classes
            .get_key_value(name)
            .map(|(key, _)| key.as_str())
            .or_else(|| {
                self.declared
                    .get_key_value(name)
                    .map(|(key, _)| key.as_str())
            })?;
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::from([name]);
        let mut found: Vec<&str> = Vec::new();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current) {
                continue;
            }
            if let Some(flags) = self.access_of(current) {
                if flags & (access::INTERFACE | access::ABSTRACT) == 0 {
                    found.push(current);
                }
            }
            if let Some(children) = self.subtypes.get(current) {
                queue.extend(children.iter().map(String::as_str));
            }
        }
        // `seen` garantit l'unicite ; l'ordre est celui des noms, comme avant.
        found.sort_unstable();
        Some(found)
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
                if self.is_fetchable(class_name) {
                    self.request(class_name);
                    return Selection::Pending;
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
        let mut pending = false;
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
                if self.is_fetchable(interface) {
                    self.request(interface);
                    pending = true;
                } else {
                    unknown = true;
                }
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
        if pending {
            return Selection::Pending;
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
pub(crate) const OBJECT: &str = "java/lang/Object";

/// Methodes d'instance de `java/lang/Object` (JDK 17), les seules qu'elle puisse fournir
/// a une selection.
pub(crate) const OBJECT_METHODS: [(&str, &str); 11] = [
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
