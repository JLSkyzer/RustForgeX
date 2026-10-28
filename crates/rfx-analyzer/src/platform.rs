//! C-07 : ce que les greffons de lancement de Forge changent au chargement (ADR-059).
//!
//! Cahier des charges : PARTIE 5.7, FM-15 ; P-02. Maturite : `STABLE`.
//!
//! Le bytecode lu dans l'archive est celui d'avant les greffons. Ce que l'analyse peut
//! reproduire exactement l'est ; ce qu'elle ne peut pas reproduire rend la classe
//! absente (son bytecode final est inconnu, FM-15). Releve dans le bytecode de Forge
//! 47.4.23 et d'eventbus 6.2.33 :
//!
//! - `RuntimeEnumExtender` reecrit `create` des enumerations qui implementent
//!   directement `IExtensibleEnum` : absente ;
//! - `CapabilityTokenSubclass` retire `final` de `CapabilityToken.getType` et ajoute
//!   `getType` a ses sous-classes directes : absentes ;
//! - eventbus ajoute des methodes et un champ a `Event` et a ses descendants : absents ;
//! - `ObjectHolderDefinalize` retire `final` des champs `@ObjectHolder` et de ceux de six
//!   classes de Minecraft : reproduit.
//!
//! `RuntimeDistCleaner` (methodes `@OnlyIn`) est traite a la resolution
//! ([`crate::hierarchy::side_conditional`]). Les noms sont ceux de la plateforme, jamais
//! d'un mod (INV-12).

use rfx_bytecode::{access, ClassFile};

use crate::hierarchy::{ClassIndex, OBJECT};

/// Classe de base des evenements d'eventbus.
pub const EVENT: &str = "net/minecraftforge/eventbus/api/Event";

/// Jeton de capacite de Forge.
pub const CAPABILITY_TOKEN: &str = "net/minecraftforge/common/capabilities/CapabilityToken";

/// Interface des enumerations extensibles de Forge.
pub const EXTENSIBLE_ENUM: &str = "net/minecraftforge/common/IExtensibleEnum";

/// Annotation des champs remplis par les registres de Forge.
pub const OBJECT_HOLDER: &str = "Lnet/minecraftforge/registries/ObjectHolder;";

/// Classes de Minecraft dont `ObjectHolderDefinalize` retire `final` de tous les champs
/// `public static final` de type objet, sans annotation.
pub const VANILLA_HOLDERS: [&str; 6] = [
    "net/minecraft/world/level/block/Blocks",
    "net/minecraft/world/item/Items",
    "net/minecraft/world/item/enchantment/Enchantments",
    "net/minecraft/world/effect/MobEffects",
    "net/minecraft/core/particles/ParticleTypes",
    "net/minecraft/sounds/SoundEvents",
];

/// `ACC_ENUM` (JVMS tableau 4.1-B).
const ACC_ENUM: u16 = 0x4000;

/// `public static final`.
const CONSTANT: u16 = access::PUBLIC | access::STATIC | access::FINAL;

/// Profondeur au-dela de laquelle une chaine de super-classes est tenue pour incoherente.
const MAX_DEPTH: usize = 256;

/// La classe telle que les greffons de Forge la laissent, ou `None` si son bytecode
/// final ne peut pas etre reproduit : elle est alors a declarer absente.
#[must_use]
pub fn as_loaded(mut class: ClassFile, index: &ClassIndex) -> Option<ClassFile> {
    let extensible_enum =
        class.access & ACC_ENUM != 0 && class.interfaces.iter().any(|i| i == EXTENSIBLE_ENUM);
    let token =
        class.name == CAPABILITY_TOKEN || class.super_name.as_deref() == Some(CAPABILITY_TOKEN);
    if extensible_enum || token || may_be_event(&class, index) {
        return None;
    }
    let vanilla_holder = VANILLA_HOLDERS.contains(&class.name.as_str());
    for field in &mut class.fields {
        let holder = field.access & CONSTANT == CONSTANT
            && field.descriptor.starts_with('L')
            && (vanilla_holder || field.annotations.iter().any(|a| a == OBJECT_HOLDER));
        if holder {
            field.access &= !access::FINAL;
        }
    }
    Some(class)
}

/// `true` si la classe est `Event`, en descend, ou si sa chaine de super-classes passe
/// par une classe inconnue : on ne peut alors pas exclure qu'elle en descende.
fn may_be_event(class: &ClassFile, index: &ClassIndex) -> bool {
    if class.name == EVENT {
        return true;
    }
    let mut last = class.name.as_str();
    let mut next = class.super_name.as_deref();
    for _ in 0..MAX_DEPTH {
        // Sans super-classe : `Object`, ou une classe dont la hierarchie est inconnue.
        let Some(current) = next else {
            return last != OBJECT;
        };
        if current == EVENT {
            return true;
        }
        if current == OBJECT {
            return false;
        }
        next = match index.superclass(current) {
            Some(parent) => parent,
            None => return true,
        };
        last = current;
    }
    true
}
