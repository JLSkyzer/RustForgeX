//! Classement des unites de travail les plus couteuses (C-35, `/rfx top`).
//!
//! Composants : C-05 (source des mesures), C-35 (exposition), C-38 (commande).
//! Cahier des charges : PARTIE 5.33.
//!
//! # Ce qui traverse la frontiere, et ce qui ne la traverse pas
//!
//! Le natif ne retient pas les noms de classes et de methodes : il ne garde que le
//! `WorkId`, calcule une fois a l'enregistrement. Ce classement ne transporte donc
//! **aucune chaine identifiant du code** — il rend des identifiants de sonde, et c'est
//! Java qui retrouve `classe#methode`, puisque c'est lui qui les a declares.
//!
//! Ce n'est pas une economie d'octets : garder les noms des deux cotes creerait deux
//! sources de verite pour la meme information, qui divergeraient au premier
//! rechargement.
//!
//! # Une mesure dit toujours d'ou elle vient
//!
//! Quand aucune sonde n'est armee, la seule mesure disponible est l'echantillonnage de
//! piles (R-322), qui est statistique. Melanger les deux dans un meme chiffre sans le
//! dire produirait un classement dont personne ne saurait ce qu'il vaut. Chaque entree
//! porte donc sa [`CostSource`].

use serde::{Deserialize, Serialize};

/// D'ou vient le cout attribue a une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CostSource {
    /// Aucune mesure : ni sonde chronometree, ni echantillon de pile.
    #[default]
    None,
    /// Sondes chronometrees : le cout est mesure a l'appel.
    Probe,
    /// Echantillonnage de piles : le cout est estime statistiquement (R-322).
    Sampling,
}

impl CostSource {
    /// Etiquette stable, destinee a l'affichage et aux rapports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "aucune",
            Self::Probe => "sonde",
            Self::Sampling => "echantillonnage",
        }
    }
}

/// Une unite de travail dans le classement.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopEntry {
    /// Identifiant de sonde ; Java y retrouve `classe#methode`.
    pub probe_id: u32,
    /// Identifiant stable de l'unite (DM-01), en hexadecimal sur seize caracteres.
    ///
    /// Texte, et non entier. Un `WorkId` est un hachage : une valeur sur deux depasse
    /// `2^63`, et le lecteur CBOR de Java refuse tout entier non signe qui ne tient pas
    /// dans un `long` — a juste titre, puisqu'il n'a pas de type non signe. Un
    /// identifiant ne sert de toute facon a aucun calcul, et la fiche normative de la
    /// PARTIE 5.33 l'affiche deja en hexadecimal.
    pub work_id_hex: String,
    /// Cout attribue par tick, en nanosecondes.
    ///
    /// C'est le critere de tri. Sa provenance est dite par [`Self::source`] : un
    /// nombre sans sa provenance ne se compare pas.
    pub cost_ns_per_tick: u64,
    /// D'ou vient [`Self::cost_ns_per_tick`].
    pub source: CostSource,
    /// Appels par tick, moyenne mobile, multipliee par cent.
    ///
    /// Entier, comme tout ce qui traverse : le decodeur CBOR de Java ne lit du type
    /// majeur 7 que les booleens. C'est un choix de conception — un decodeur minimal a
    /// une surface d'attaque minimale — et c'est pourquoi `RuntimeStatus` ecrit deja
    /// `overhead_pct_x100`. Un reel serialise ici rendrait tout le blob illisible.
    pub calls_per_tick_x100: u64,
    /// Temps processeur par appel, mediane, en nanosecondes.
    pub cpu_ns_p50: u64,
    /// Temps processeur par appel, 95e centile, en nanosecondes.
    pub cpu_ns_p95: u64,
    /// Appels chronometres depuis le demarrage.
    ///
    /// Zero signifie qu'aucune sonde n'a jamais rapporte pour cette unite ; les
    /// centiles ci-dessus ne veulent alors rien dire, et l'afficheur doit le savoir.
    pub timed_calls: u64,
    /// Echantillons de pile ayant designe cette unite.
    pub sampled_hits: u64,
    /// Octets alloues par tick, moyenne mobile, arrondie.
    pub alloc_bytes_per_tick: u64,
    /// Classe de cout.
    pub heat: String,
    /// Niveau de sonde courant.
    pub level: String,
    /// Confiance dans l'observation, en pourcent, de `0` a `100`.
    pub observation_quality_pct: u32,
}

/// Classement complet, tel qu'il traverse la frontiere.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopWorkloads {
    /// Version du format, pour qu'un Java plus ancien sache refuser.
    pub schema: u32,
    /// Unites vivantes au moment du relevé, toutes, pas seulement les listees.
    pub tracked: u64,
    /// Unites ayant un cout non nul, quelle qu'en soit la source.
    ///
    /// Compare a `tracked`, ce nombre dit si le classement est representatif ou si
    /// presque rien n'a ete observe.
    pub measured: u64,
    /// Duree du dernier tick observe, en nanosecondes.
    ///
    /// Transmise avec le classement pour que l'afficheur puisse rendre une **part du
    /// tick** plutot que des microsecondes brutes. « 268 us » ne dit rien a qui exploite
    /// un serveur ; « 6 % du tick » dit quoi faire.
    ///
    /// Zero signifie qu'aucun tick n'a encore ete cloture : l'afficheur doit alors
    /// s'abstenir de calculer une part, et non diviser par zero ni inventer un total.
    pub tick_ns: u64,
    /// Les entrees, de la plus couteuse a la moins couteuse.
    pub entries: Vec<TopEntry>,
}

impl TopWorkloads {
    /// Version courante du format.
    pub const SCHEMA: u32 = 1;
}

/// Verifie qu'un blob CBOR tient dans le sous-ensemble que Java sait lire.
///
/// # Pourquoi cette fonction n'est pas un test
///
/// Le lecteur de `dev.rustforgex.bridge.CborReader` est volontairement minimal :
/// entiers positifs et negatifs, chaines d'octets et de texte, tableaux, tables, et du
/// type majeur 7 les seuls booleens. Tout le reste leve, et **le blob entier devient
/// illisible** — pas seulement le champ fautif.
///
/// Trois defauts de cette famille ont ete trouves en production le meme jour : un `f64`
/// dans le classement, un `u64` au-dela de `2^63` pour un identifiant, et une
/// difference signee dans le statut. Chacun rendait muet tout un pan du mod, sans
/// message, et aucun test ne les a vus. Ce controle vit donc **hors des tests**, pour
/// que n'importe quel crate puisse verifier son propre modele.
///
/// # Erreurs
///
/// Rend le motif du refus, formule pour dire quoi corriger.
pub fn assert_java_readable(blob: &[u8]) -> Result<(), String> {
    let mut cursor = 0_usize;
    walk_java_readable(blob, &mut cursor)?;
    if cursor != blob.len() {
        return Err(format!(
            "{} octets non consommes en fin de blob",
            blob.len() - cursor
        ));
    }
    Ok(())
}

/// Parcourt une valeur CBOR en refusant tout ce que Java refuserait.
fn walk_java_readable(blob: &[u8], cursor: &mut usize) -> Result<(), String> {
    if *cursor >= blob.len() {
        return Err("blob tronque".to_owned());
    }
    let initial = blob[*cursor];
    *cursor += 1;
    let major = initial >> 5;
    let info = initial & 0x1f;

    if major == 7 {
        if info == 20 || info == 21 {
            return Ok(());
        }
        return Err(format!(
            "type majeur 7 avec info {info} : Java ne lit que les booleens, jamais un              reel. Exprimer la valeur en entier, comme `overhead_pct_x100`."
        ));
    }

    let argument = match info {
        0..=23 => u64::from(info),
        24 => read_be(blob, cursor, 1)?,
        25 => read_be(blob, cursor, 2)?,
        26 => read_be(blob, cursor, 4)?,
        27 => read_be(blob, cursor, 8)?,
        other => return Err(format!("argument CBOR non supporte par Java : {other}")),
    };

    match major {
        0 | 1 => {
            if argument > i64::MAX as u64 {
                return Err(format!(
                    "entier {argument} au-dela de 2^63 : Java n'a pas de type non signe                      et refusera tout le blob. Le transmettre en texte, comme                      `work_id_hex`."
                ));
            }
            Ok(())
        }
        2 | 3 => {
            let len = usize::try_from(argument).map_err(|_| "longueur hors bornes")?;
            *cursor += len;
            if *cursor > blob.len() {
                return Err("chaine debordant du blob".to_owned());
            }
            Ok(())
        }
        4 => {
            for _ in 0..argument {
                walk_java_readable(blob, cursor)?;
            }
            Ok(())
        }
        5 => {
            for _ in 0..argument {
                walk_java_readable(blob, cursor)?; // cle
                walk_java_readable(blob, cursor)?; // valeur
            }
            Ok(())
        }
        other => Err(format!("type majeur CBOR non supporte par Java : {other}")),
    }
}

fn read_be(blob: &[u8], cursor: &mut usize, width: usize) -> Result<u64, String> {
    if *cursor + width > blob.len() {
        return Err("argument tronque".to_owned());
    }
    let mut value = 0_u64;
    for _ in 0..width {
        value = (value << 8) | u64::from(blob[*cursor]);
        *cursor += 1;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raccourci de test : echoue en nommant le motif du refus.
    fn assert_readable(blob: &[u8]) {
        if let Err(reason) = super::assert_java_readable(blob) {
            panic!("{reason}");
        }
    }

    /// Le classement doit rester dans le sous-ensemble CBOR que Java sait lire.
    ///
    /// Ce test nait d'un defaut reel : la premiere version portait `calls_per_tick` en
    /// `f64`. Le blob etait parfaitement valide, et parfaitement indechiffrable pour le
    /// mod — `/rfx top` repondait « aucun classement disponible » alors que le natif en
    /// avait produit un. Rien ne l'a signale avant un lancement de serveur.
    #[test]
    fn the_ranking_stays_within_what_java_can_read() {
        let top = TopWorkloads {
            schema: TopWorkloads::SCHEMA,
            tick_ns: 20_821_000,
            tracked: 2_600,
            measured: 12,
            entries: vec![TopEntry {
                probe_id: 42,
                work_id_hex: "9e3779b97f4a7c15".to_owned(),
                cost_ns_per_tick: 51_800,
                source: CostSource::Probe,
                calls_per_tick_x100: 41_200,
                cpu_ns_p50: 14_200,
                cpu_ns_p95: 51_800,
                timed_calls: 900,
                sampled_hits: 3,
                alloc_bytes_per_tick: 2_100_000,
                heat: "CRITICAL".to_owned(),
                level: "TIMED".to_owned(),
                observation_quality_pct: 94,
            }],
        };

        let blob = crate::to_cbor(&top).expect("serialisation");
        assert_readable(&blob);
    }

    /// Le garde-fou doit attraper le defaut qu'il pretend prevenir.
    ///
    /// Sa premiere version ne verifiait que la FORME du CBOR — types majeurs, longueurs
    /// — et laissait passer un `u64` au-dela de `2^63`. Le classement portait un
    /// `work_id: u64`, donc un hachage : une invocation de `/rfx top` sur deux rendait
    /// un blob que Java refusait en entier. Un garde-fou qu'on ne met pas en echec ne
    /// prouve rien.
    #[test]
    #[should_panic(expected = "au-dela de 2^63")]
    fn the_guard_catches_an_unsigned_integer_java_cannot_read() {
        // 0x1b suivi de huit octets : entier non signe sur soixante-quatre bits.
        let blob = [0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert_readable(&blob);
    }

    /// Le classement vide traverse aussi : c'est le cas le plus frequent au demarrage.
    ///
    /// La contrainte vaut pour **tout** modele traversant la frontiere, pas seulement
    /// pour celui-ci ; `RuntimeStatus` la respecte deja, par la convention `_x100`.
    #[test]
    fn an_empty_ranking_also_crosses() {
        let blob = crate::to_cbor(&TopWorkloads::default()).expect("serialisation");
        assert_readable(&blob);
    }
}
