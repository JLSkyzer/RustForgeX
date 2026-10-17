//! Fichier `.class` (JVMS §4.1) : hierarchie, methodes, code, methodes d'amorcage.

use crate::code::{self, CallSite};
use crate::constant::ConstantPool;
use crate::cursor::Cursor;
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
    /// `ACC_NATIVE`.
    pub const NATIVE: u16 = 0x0100;
    /// `ACC_INTERFACE`.
    pub const INTERFACE: u16 = 0x0200;
    /// `ACC_ABSTRACT`.
    pub const ABSTRACT: u16 = 0x0400;
    /// `ACC_SYNTHETIC`.
    pub const SYNTHETIC: u16 = 0x1000;
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
    /// Methodes declarees.
    pub methods: Vec<MethodInfo>,
    /// Methodes d'amorcage des `invokedynamic`.
    pub bootstrap_methods: Vec<BootstrapMethod>,
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

        // Les champs ne servent pas a l'analyse des appels : ils sont sautes.
        let field_count = usize::from(cursor.u16()?);
        for _ in 0..field_count {
            cursor.take(6)?;
            skip_attributes(&mut cursor)?;
        }

        let method_count = usize::from(cursor.u16()?);
        let mut methods = Vec::with_capacity(cursor.capacity_for(method_count, 8));
        for _ in 0..method_count {
            methods.push(parse_method(&mut cursor, &pool)?);
        }

        let mut bootstrap_methods = Vec::new();
        let attribute_count = usize::from(cursor.u16()?);
        for _ in 0..attribute_count {
            let attribute_name = pool.utf8(cursor.u16()?)?;
            let len = cursor.u32()? as usize;
            let body = cursor.take(len)?;
            if attribute_name == "BootstrapMethods" {
                bootstrap_methods = parse_bootstrap_methods(body)?;
            }
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
            methods,
            bootstrap_methods,
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
}

fn skip_attributes(cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    let count = cursor.u16()?;
    for _ in 0..count {
        cursor.take(2)?;
        let len = cursor.u32()? as usize;
        cursor.take(len)?;
    }
    Ok(())
}

fn parse_method(cursor: &mut Cursor<'_>, pool: &ConstantPool) -> Result<MethodInfo, ParseError> {
    let access = cursor.u16()?;
    let name = pool.utf8(cursor.u16()?)?.to_owned();
    let descriptor = pool.utf8(cursor.u16()?)?.to_owned();
    let mut code = None;
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
            code = Some(parse_code(body)?);
        }
    }
    Ok(MethodInfo {
        access,
        name,
        descriptor,
        code,
    })
}

/// Attribut `Code` (JVMS §4.7.3) : seul le code est garde.
fn parse_code(body: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut cursor = Cursor::new(body);
    cursor.u16()?; // max_stack
    cursor.u16()?; // max_locals
    let len = cursor.u32()? as usize;
    // JVMS §4.7.3 : 0 < code_length < 65536.
    if len == 0 || len >= 65_536 {
        return Err(ParseError::Malformed("longueur de code hors de 1 a 65535"));
    }
    Ok(cursor.take(len)?.to_vec())
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
