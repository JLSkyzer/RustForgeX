//! Fichier `.class` (JVMS §4.1) : hierarchie, methodes, code, methodes d'amorcage,
//! types des annotations.

use crate::code::{self, CallSite};
use crate::constant::ConstantPool;
use crate::cursor::Cursor;
use crate::insn::{self, Handler, Instruction};
use crate::ParseError;

/// Drapeaux d'acces (JVMS tableaux 4.1-B et 4.6-A).
pub mod access {
    /// `ACC_PUBLIC`.
    pub const PUBLIC: u16 = 0x0001;
    /// `ACC_PRIVATE`.
    pub const PRIVATE: u16 = 0x0002;
    /// `ACC_PROTECTED`.
    pub const PROTECTED: u16 = 0x0004;
    /// `ACC_STATIC`.
    pub const STATIC: u16 = 0x0008;
    /// `ACC_FINAL`.
    pub const FINAL: u16 = 0x0010;
    /// `ACC_SYNCHRONIZED`, sur une methode (sur une classe, le meme bit est `ACC_SUPER`).
    pub const SYNCHRONIZED: u16 = 0x0020;
    /// `ACC_VOLATILE`, sur un champ (sur une methode, le meme bit est `ACC_BRIDGE`).
    pub const VOLATILE: u16 = 0x0040;
    /// `ACC_NATIVE`.
    pub const NATIVE: u16 = 0x0100;
    /// `ACC_INTERFACE`.
    pub const INTERFACE: u16 = 0x0200;
    /// `ACC_ABSTRACT`.
    pub const ABSTRACT: u16 = 0x0400;
    /// `ACC_SYNTHETIC`.
    pub const SYNTHETIC: u16 = 0x1000;
}

/// Un champ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Drapeaux d'acces.
    pub access: u16,
    /// Nom.
    pub name: String,
    /// Descripteur (`I`, `Ljava/util/List;`).
    pub descriptor: String,
    /// Types des annotations, visibles ou non a l'execution (`Lpkg/Annotation;`).
    pub annotations: Vec<String>,
}

impl FieldInfo {
    /// `true` si le champ est statique.
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.access & access::STATIC != 0
    }

    /// `true` si le champ peut changer apres la construction : ni `final`, ni
    /// `volatile` (dont l'ecriture est visible des autres fils).
    #[must_use]
    pub fn is_plain_mutable(&self) -> bool {
        self.access & (access::FINAL | access::VOLATILE) == 0
    }
}

/// Une methode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// Drapeaux d'acces.
    pub access: u16,
    /// Nom (`tick`, `<init>`, `<clinit>`).
    pub name: String,
    /// Descripteur (`(I)V`).
    pub descriptor: String,
    /// Code, absent pour une methode abstraite ou native.
    pub code: Option<Vec<u8>>,
    /// Hauteur maximale de la pile, en emplacements ; 0 sans code.
    pub max_stack: u16,
    /// Nombre de variables locales, en emplacements ; 0 sans code.
    pub max_locals: u16,
    /// Gestionnaires d'exceptions du code, dans l'ordre de la table.
    pub handlers: Vec<Handler>,
    /// Types des annotations, visibles ou non a l'execution (`Lpkg/Annotation;`).
    pub annotations: Vec<String>,
}

impl MethodInfo {
    /// `true` si la methode est statique.
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.access & access::STATIC != 0
    }

    /// `true` si la methode est abstraite.
    #[must_use]
    pub fn is_abstract(&self) -> bool {
        self.access & access::ABSTRACT != 0
    }

    /// `true` si la methode est privee.
    #[must_use]
    pub fn is_private(&self) -> bool {
        self.access & access::PRIVATE != 0
    }

    /// `true` si la methode est finale.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.access & access::FINAL != 0
    }
}

/// Methode d'amorcage d'un `invokedynamic` (attribut `BootstrapMethods`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapMethod {
    /// Indice du `MethodHandle` d'amorcage.
    pub method: u16,
    /// Indices des arguments statiques.
    pub arguments: Vec<u16>,
}

/// Un fichier `.class` lu.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassFile {
    /// Version mineure du format.
    pub minor_version: u16,
    /// Version majeure du format (61 pour Java 17).
    pub major_version: u16,
    /// Drapeaux d'acces de la classe.
    pub access: u16,
    /// Nom interne (`net/minecraft/world/entity/Mob`).
    pub name: String,
    /// Super-classe ; absente pour `java/lang/Object` seulement.
    pub super_name: Option<String>,
    /// Interfaces directement implementees.
    pub interfaces: Vec<String>,
    /// Champs declares.
    pub fields: Vec<FieldInfo>,
    /// Methodes declarees.
    pub methods: Vec<MethodInfo>,
    /// Methodes d'amorcage des `invokedynamic`.
    pub bootstrap_methods: Vec<BootstrapMethod>,
    /// Types des annotations de la classe, visibles ou non a l'execution.
    pub annotations: Vec<String>,
    /// La classe appartient a un nid (attribut `NestHost` ou `NestMembers`, JVMS
    /// §4.7.28-29) : d'autres classes peuvent ecrire ses champs prives.
    pub nested: bool,
    pool: ConstantPool,
}

/// Signature d'un fichier `.class`.
const MAGIC: u32 = 0xCAFE_BABE;

impl ClassFile {
    /// Lit un fichier `.class` entier.
    ///
    /// # Errors
    ///
    /// [`ParseError`] si le fichier est tronque, incoherent, ou suivi d'octets en trop.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.u32().map_err(|_| ParseError::BadMagic)? != MAGIC {
            return Err(ParseError::BadMagic);
        }
        let minor_version = cursor.u16()?;
        let major_version = cursor.u16()?;
        let pool = ConstantPool::parse(&mut cursor)?;
        let access = cursor.u16()?;
        let name = pool.class_name(cursor.u16()?)?.to_owned();
        let super_name = match cursor.u16()? {
            0 => None,
            index => Some(pool.class_name(index)?.to_owned()),
        };

        let interface_count = usize::from(cursor.u16()?);
        let mut interfaces = Vec::with_capacity(cursor.capacity_for(interface_count, 2));
        for _ in 0..interface_count {
            interfaces.push(pool.class_name(cursor.u16()?)?.to_owned());
        }

        // Champs : drapeaux, nom, descripteur et annotations ; les autres attributs sont
        // sautes.
        let field_count = usize::from(cursor.u16()?);
        let mut fields = Vec::with_capacity(cursor.capacity_for(field_count, 8));
        for _ in 0..field_count {
            let access = cursor.u16()?;
            let name = pool.utf8(cursor.u16()?)?.to_owned();
            let descriptor = pool.utf8(cursor.u16()?)?.to_owned();
            let mut annotations = Vec::new();
            let attribute_count = cursor.u16()?;
            for _ in 0..attribute_count {
                let attribute_name = pool.utf8(cursor.u16()?)?;
                let len = cursor.u32()? as usize;
                let body = cursor.take(len)?;
                if is_annotation_attribute(attribute_name) {
                    annotations.extend(parse_annotation_types(body, &pool)?);
                }
            }
            fields.push(FieldInfo {
                access,
                name,
                descriptor,
                annotations,
            });
        }

        let method_count = usize::from(cursor.u16()?);
        let mut methods = Vec::with_capacity(cursor.capacity_for(method_count, 8));
        for _ in 0..method_count {
            methods.push(parse_method(&mut cursor, &pool)?);
        }

        let mut bootstrap_methods = Vec::new();
        let mut annotations = Vec::new();
        let mut nested = false;
        let attribute_count = usize::from(cursor.u16()?);
        for _ in 0..attribute_count {
            let attribute_name = pool.utf8(cursor.u16()?)?;
            let len = cursor.u32()? as usize;
            let body = cursor.take(len)?;
            if attribute_name == "BootstrapMethods" {
                bootstrap_methods = parse_bootstrap_methods(body)?;
            } else if is_annotation_attribute(attribute_name) {
                annotations.extend(parse_annotation_types(body, &pool)?);
            }
            nested |= attribute_name == "NestHost" || attribute_name == "NestMembers";
        }
        if cursor.remaining() != 0 {
            return Err(ParseError::Malformed("octets apres la fin de la classe"));
        }

        Ok(Self {
            minor_version,
            major_version,
            access,
            name,
            super_name,
            interfaces,
            fields,
            methods,
            bootstrap_methods,
            annotations,
            nested,
            pool,
        })
    }

    /// Reservoir de constantes.
    #[must_use]
    pub fn pool(&self) -> &ConstantPool {
        &self.pool
    }

    /// Methode declaree par son nom et son descripteur.
    #[must_use]
    pub fn method(&self, name: &str, descriptor: &str) -> Option<&MethodInfo> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)
    }

    /// `true` si la classe, ou un de ses champs ou une de ses methodes, porte une
    /// annotation de ce type (`Lpkg/Annotation;`).
    #[must_use]
    pub fn mentions_annotation(&self, descriptor: &str) -> bool {
        let named = |list: &[String]| list.iter().any(|a| a == descriptor);
        named(&self.annotations)
            || self.fields.iter().any(|f| named(&f.annotations))
            || self.methods.iter().any(|m| named(&m.annotations))
    }

    /// `true` si c'est une interface.
    #[must_use]
    pub fn is_interface(&self) -> bool {
        self.access & access::INTERFACE != 0
    }

    /// `true` si la classe est abstraite (une interface l'est toujours).
    #[must_use]
    pub fn is_abstract(&self) -> bool {
        self.access & access::ABSTRACT != 0
    }

    /// `true` si la classe est finale.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.access & access::FINAL != 0
    }

    /// Sites d'appel d'une methode de cette classe, dans l'ordre du code.
    ///
    /// # Errors
    ///
    /// [`ParseError`] si le code ou une constante qu'il designe est incoherent.
    pub fn call_sites(&self, method: &MethodInfo) -> Result<Vec<CallSite>, ParseError> {
        match &method.code {
            Some(code) => code::call_sites(code, &self.pool, &self.bootstrap_methods),
            None => Ok(Vec::new()),
        }
    }

    /// Instructions d'une methode de cette classe, dans l'ordre du code (C-11).
    ///
    /// # Errors
    ///
    /// [`ParseError`] si le code ou une constante qu'il designe est incoherent, ou si
    /// le code contient `jsr` ou `ret`.
    pub fn instructions(&self, method: &MethodInfo) -> Result<Vec<Instruction>, ParseError> {
        match &method.code {
            Some(code) => insn::decode(code, &self.pool, &self.bootstrap_methods),
            None => Ok(Vec::new()),
        }
    }
}

/// Profondeur d'imbrication des valeurs d'annotation au-dela de laquelle le fichier est
/// tenu pour mal forme : une pile bornee, quel que soit le fichier lu.
const MAX_ANNOTATION_DEPTH: u32 = 64;

/// Attributs d'annotations dont on garde les types (JVMS §4.7.16-17). Les annotations
/// invisibles a l'execution comptent aussi : un greffon de lancement lit le fichier,
/// pas la reflexion.
fn is_annotation_attribute(name: &str) -> bool {
    name == "RuntimeVisibleAnnotations" || name == "RuntimeInvisibleAnnotations"
}

/// Types des annotations d'un attribut `Runtime(In)VisibleAnnotations`.
fn parse_annotation_types(body: &[u8], pool: &ConstantPool) -> Result<Vec<String>, ParseError> {
    let mut cursor = Cursor::new(body);
    let count = usize::from(cursor.u16()?);
    let mut types = Vec::with_capacity(cursor.capacity_for(count, 4));
    for _ in 0..count {
        types.push(pool.utf8(cursor.u16()?)?.to_owned());
        skip_element_pairs(&mut cursor, 0)?;
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::Malformed("octets apres les annotations"));
    }
    Ok(types)
}

/// Saute les paires `nom = valeur` d'une annotation (JVMS §4.7.16).
fn skip_element_pairs(cursor: &mut Cursor<'_>, depth: u32) -> Result<(), ParseError> {
    let pairs = cursor.u16()?;
    for _ in 0..pairs {
        cursor.take(2)?;
        skip_element_value(cursor, depth)?;
    }
    Ok(())
}

/// Saute une valeur d'element (JVMS §4.7.16.1).
fn skip_element_value(cursor: &mut Cursor<'_>, depth: u32) -> Result<(), ParseError> {
    if depth >= MAX_ANNOTATION_DEPTH {
        return Err(ParseError::Malformed("annotations trop imbriquees"));
    }
    match cursor.u8()? {
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b's' | b'c' => {
            cursor.take(2)?;
        }
        b'e' => {
            cursor.take(4)?;
        }
        b'@' => {
            cursor.take(2)?;
            skip_element_pairs(cursor, depth + 1)?;
        }
        b'[' => {
            let values = cursor.u16()?;
            for _ in 0..values {
                skip_element_value(cursor, depth + 1)?;
            }
        }
        _ => return Err(ParseError::Malformed("valeur d'annotation inconnue")),
    }
    Ok(())
}

fn parse_method(cursor: &mut Cursor<'_>, pool: &ConstantPool) -> Result<MethodInfo, ParseError> {
    let access = cursor.u16()?;
    let name = pool.utf8(cursor.u16()?)?.to_owned();
    let descriptor = pool.utf8(cursor.u16()?)?.to_owned();
    let mut code: Option<Code> = None;
    let mut annotations = Vec::new();
    let attribute_count = cursor.u16()?;
    for _ in 0..attribute_count {
        let attribute_name = pool.utf8(cursor.u16()?)?;
        let len = cursor.u32()? as usize;
        // Chaque attribut est lu dans sa propre tranche : un attribut mal forme ne
        // decale pas la lecture du reste.
        let body = cursor.take(len)?;
        if attribute_name == "Code" {
            if code.is_some() {
                return Err(ParseError::Malformed("deux attributs Code"));
            }
            code = Some(parse_code(body, pool)?);
        } else if is_annotation_attribute(attribute_name) {
            annotations.extend(parse_annotation_types(body, pool)?);
        }
    }
    let code = code.unwrap_or_default();
    Ok(MethodInfo {
        access,
        name,
        descriptor,
        code: code.bytes,
        max_stack: code.max_stack,
        max_locals: code.max_locals,
        handlers: code.handlers,
        annotations,
    })
}

/// Contenu lu d'un attribut `Code`.
#[derive(Default)]
struct Code {
    bytes: Option<Vec<u8>>,
    max_stack: u16,
    max_locals: u16,
    handlers: Vec<Handler>,
}

/// Attribut `Code` (JVMS §4.7.3) : code, bornes et table des exceptions ; ses propres
/// attributs (`LineNumberTable`, `StackMapTable`…) sont sautes.
fn parse_code(body: &[u8], pool: &ConstantPool) -> Result<Code, ParseError> {
    let mut cursor = Cursor::new(body);
    let max_stack = cursor.u16()?;
    let max_locals = cursor.u16()?;
    let len = cursor.u32()? as usize;
    // JVMS §4.7.3 : 0 < code_length < 65536.
    if len == 0 || len >= 65_536 {
        return Err(ParseError::Malformed("longueur de code hors de 1 a 65535"));
    }
    let bytes = cursor.take(len)?.to_vec();
    let count = usize::from(cursor.u16()?);
    let mut handlers = Vec::with_capacity(cursor.capacity_for(count, 8));
    for _ in 0..count {
        let start = u32::from(cursor.u16()?);
        let end = u32::from(cursor.u16()?);
        let handler = u32::from(cursor.u16()?);
        let catch_type = match cursor.u16()? {
            0 => None,
            index => Some(pool.class_name(index)?.to_owned()),
        };
        handlers.push(Handler {
            start,
            end,
            handler,
            catch_type,
        });
    }
    Ok(Code {
        bytes: Some(bytes),
        max_stack,
        max_locals,
        handlers,
    })
}

fn parse_bootstrap_methods(body: &[u8]) -> Result<Vec<BootstrapMethod>, ParseError> {
    let mut cursor = Cursor::new(body);
    let count = usize::from(cursor.u16()?);
    let mut methods = Vec::with_capacity(cursor.capacity_for(count, 4));
    for _ in 0..count {
        let method = cursor.u16()?;
        let argument_count = usize::from(cursor.u16()?);
        let mut arguments = Vec::with_capacity(cursor.capacity_for(argument_count, 2));
        for _ in 0..argument_count {
            arguments.push(cursor.u16()?);
        }
        methods.push(BootstrapMethod { method, arguments });
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::Malformed(
            "BootstrapMethods plus long que son contenu",
        ));
    }
    Ok(methods)
}
