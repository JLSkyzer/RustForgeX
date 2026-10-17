//! Reservoir de constantes (JVMS §4.4).
//!
//! Les references entre constantes ne sont verifiees qu'a l'usage : une constante
//! jamais designee peut etre incoherente sans empecher la lecture du reste.

use crate::cursor::Cursor;
use crate::mutf8;
use crate::ParseError;

/// Une constante.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    /// Chaine en UTF-8 modifie, decodee.
    Utf8(String),
    /// Entier sur 32 bits.
    Integer(i32),
    /// Flottant sur 32 bits, tel qu'ecrit.
    Float(u32),
    /// Entier sur 64 bits.
    Long(i64),
    /// Flottant sur 64 bits, tel qu'ecrit.
    Double(u64),
    /// Classe ou interface, par son nom interne.
    Class {
        /// Indice du nom.
        name: u16,
    },
    /// Litteral de chaine.
    String {
        /// Indice de la chaine.
        value: u16,
    },
    /// Champ.
    FieldRef {
        /// Indice de la classe.
        class: u16,
        /// Indice du nom et du type.
        name_and_type: u16,
    },
    /// Methode d'une classe.
    MethodRef {
        /// Indice de la classe.
        class: u16,
        /// Indice du nom et du type.
        name_and_type: u16,
    },
    /// Methode d'une interface.
    InterfaceMethodRef {
        /// Indice de l'interface.
        class: u16,
        /// Indice du nom et du type.
        name_and_type: u16,
    },
    /// Nom et descripteur.
    NameAndType {
        /// Indice du nom.
        name: u16,
        /// Indice du descripteur.
        descriptor: u16,
    },
    /// Handle de methode.
    MethodHandle {
        /// Nature de la reference (JVMS §5.4.3.5), de 1 a 9.
        kind: u8,
        /// Indice du membre designe.
        reference: u16,
    },
    /// Type de methode.
    MethodType {
        /// Indice du descripteur.
        descriptor: u16,
    },
    /// Constante calculee dynamiquement.
    Dynamic {
        /// Indice dans `BootstrapMethods`.
        bootstrap: u16,
        /// Indice du nom et du type.
        name_and_type: u16,
    },
    /// Site d'appel `invokedynamic`.
    InvokeDynamic {
        /// Indice dans `BootstrapMethods`.
        bootstrap: u16,
        /// Indice du nom et du type.
        name_and_type: u16,
    },
    /// Module.
    Module {
        /// Indice du nom.
        name: u16,
    },
    /// Paquet.
    Package {
        /// Indice du nom.
        name: u16,
    },
    /// Emplacement 0, ou seconde moitie d'une constante longue : jamais designable.
    Unusable,
}

/// Membre designe : classe proprietaire, nom, descripteur.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberRef {
    /// Nom interne de la classe proprietaire (`net/minecraft/world/entity/Mob`).
    pub owner: String,
    /// Nom du membre.
    pub name: String,
    /// Descripteur (`(I)V`).
    pub descriptor: String,
    /// `true` si la reference passe par une interface.
    pub interface: bool,
}

/// Handle de methode resolu.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodHandleRef {
    /// Nature de la reference : 5 `invokeVirtual`, 6 `invokeStatic`, 7 `invokeSpecial`,
    /// 8 `newInvokeSpecial`, 9 `invokeInterface` ; 1 a 4 designent des champs.
    pub kind: u8,
    /// Membre designe.
    pub member: MemberRef,
}

/// Reservoir de constantes d'une classe.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantPool {
    entries: Vec<Constant>,
}

impl ConstantPool {
    pub(crate) fn parse(cursor: &mut Cursor<'_>) -> Result<Self, ParseError> {
        let count = cursor.u16()?;
        if count == 0 {
            return Err(ParseError::Malformed("reservoir de constantes vide"));
        }
        // Une constante occupe au moins trois octets.
        let mut entries = Vec::with_capacity(cursor.capacity_for(usize::from(count), 3));
        entries.push(Constant::Unusable);
        let mut index: u16 = 1;
        while index < count {
            let tag = cursor.u8()?;
            let constant = match tag {
                1 => {
                    let len = usize::from(cursor.u16()?);
                    let bytes = cursor.take(len)?;
                    Constant::Utf8(mutf8::decode(bytes).ok_or(ParseError::InvalidUtf8 { index })?)
                }
                3 => Constant::Integer(i32::from_be_bytes(cursor.u32()?.to_be_bytes())),
                4 => Constant::Float(cursor.u32()?),
                5 | 6 => {
                    let high = u64::from(cursor.u32()?);
                    let low = u64::from(cursor.u32()?);
                    let bits = (high << 32) | low;
                    if tag == 5 {
                        Constant::Long(i64::from_be_bytes(bits.to_be_bytes()))
                    } else {
                        Constant::Double(bits)
                    }
                }
                7 => Constant::Class {
                    name: cursor.u16()?,
                },
                8 => Constant::String {
                    value: cursor.u16()?,
                },
                9..=11 => {
                    let class = cursor.u16()?;
                    let name_and_type = cursor.u16()?;
                    match tag {
                        9 => Constant::FieldRef {
                            class,
                            name_and_type,
                        },
                        10 => Constant::MethodRef {
                            class,
                            name_and_type,
                        },
                        _ => Constant::InterfaceMethodRef {
                            class,
                            name_and_type,
                        },
                    }
                }
                12 => Constant::NameAndType {
                    name: cursor.u16()?,
                    descriptor: cursor.u16()?,
                },
                15 => Constant::MethodHandle {
                    kind: cursor.u8()?,
                    reference: cursor.u16()?,
                },
                16 => Constant::MethodType {
                    descriptor: cursor.u16()?,
                },
                17 | 18 => {
                    let bootstrap = cursor.u16()?;
                    let name_and_type = cursor.u16()?;
                    if tag == 17 {
                        Constant::Dynamic {
                            bootstrap,
                            name_and_type,
                        }
                    } else {
                        Constant::InvokeDynamic {
                            bootstrap,
                            name_and_type,
                        }
                    }
                }
                19 => Constant::Module {
                    name: cursor.u16()?,
                },
                20 => Constant::Package {
                    name: cursor.u16()?,
                },
                _ => return Err(ParseError::UnknownTag { index, tag }),
            };
            let wide = matches!(constant, Constant::Long(_) | Constant::Double(_));
            entries.push(constant);
            index += 1;
            if wide {
                // JVMS §4.4.5 : une constante longue occupe deux emplacements.
                if index >= count {
                    return Err(ParseError::Malformed(
                        "constante longue en fin de reservoir",
                    ));
                }
                entries.push(Constant::Unusable);
                index += 1;
            }
        }
        Ok(Self { entries })
    }

    /// Nombre d'emplacements, emplacement 0 compris.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` si le reservoir ne contient que l'emplacement 0.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.len() <= 1
    }

    /// Constante a un indice.
    ///
    /// # Errors
    ///
    /// [`ParseError::BadIndex`] si l'indice ne designe pas une constante.
    pub fn get(&self, index: u16) -> Result<&Constant, ParseError> {
        match self.entries.get(usize::from(index)) {
            Some(Constant::Unusable) | None => Err(ParseError::BadIndex { index }),
            Some(constant) => Ok(constant),
        }
    }

    /// Chaine a un indice.
    ///
    /// # Errors
    ///
    /// Si l'indice ne designe pas une chaine.
    pub fn utf8(&self, index: u16) -> Result<&str, ParseError> {
        match self.get(index)? {
            Constant::Utf8(text) => Ok(text),
            _ => Err(ParseError::WrongKind {
                index,
                expected: "Utf8",
            }),
        }
    }

    /// Nom interne de la classe a un indice.
    ///
    /// # Errors
    ///
    /// Si l'indice ne designe pas une classe.
    pub fn class_name(&self, index: u16) -> Result<&str, ParseError> {
        match self.get(index)? {
            Constant::Class { name } => self.utf8(*name),
            _ => Err(ParseError::WrongKind {
                index,
                expected: "Class",
            }),
        }
    }

    /// Nom et descripteur a un indice.
    ///
    /// # Errors
    ///
    /// Si l'indice ne designe pas un `NameAndType`.
    pub fn name_and_type(&self, index: u16) -> Result<(&str, &str), ParseError> {
        match self.get(index)? {
            Constant::NameAndType { name, descriptor } => {
                Ok((self.utf8(*name)?, self.utf8(*descriptor)?))
            }
            _ => Err(ParseError::WrongKind {
                index,
                expected: "NameAndType",
            }),
        }
    }

    /// Membre (champ ou methode) a un indice.
    ///
    /// # Errors
    ///
    /// Si l'indice ne designe pas une reference de membre.
    pub fn member(&self, index: u16) -> Result<MemberRef, ParseError> {
        let (class, name_and_type, interface) = match self.get(index)? {
            Constant::FieldRef {
                class,
                name_and_type,
            }
            | Constant::MethodRef {
                class,
                name_and_type,
            } => (*class, *name_and_type, false),
            Constant::InterfaceMethodRef {
                class,
                name_and_type,
            } => (*class, *name_and_type, true),
            _ => {
                return Err(ParseError::WrongKind {
                    index,
                    expected: "Fieldref, Methodref ou InterfaceMethodref",
                })
            }
        };
        let (name, descriptor) = self.name_and_type(name_and_type)?;
        Ok(MemberRef {
            owner: self.class_name(class)?.to_owned(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
            interface,
        })
    }

    /// Handle de methode a un indice.
    ///
    /// # Errors
    ///
    /// Si l'indice ne designe pas un `MethodHandle` coherent.
    pub fn method_handle(&self, index: u16) -> Result<MethodHandleRef, ParseError> {
        match self.get(index)? {
            Constant::MethodHandle { kind, reference } => {
                if !(1..=9).contains(kind) {
                    return Err(ParseError::Malformed("nature de handle hors de 1 a 9"));
                }
                Ok(MethodHandleRef {
                    kind: *kind,
                    member: self.member(*reference)?,
                })
            }
            _ => Err(ParseError::WrongKind {
                index,
                expected: "MethodHandle",
            }),
        }
    }
}
