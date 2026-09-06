package rfxbench.synth;

/**
 * Une unité de travail synthétique générée (PARTIE 22).
 *
 * <p>Les implémentations sont <strong>engendrées au build</strong>, pas écrites à la
 * main : ce qu'on cherche à reproduire est la <em>largeur</em> d'un modpack — des
 * centaines de classes, des milliers de méthodes assez longues pour être sondées — et
 * cela ne s'écrit pas au clavier.
 *
 * <h2>Pourquoi la largeur, et pas seulement la charge</h2>
 *
 * <p>Le premier mod synthétique tickait fort mais n'apportait que <strong>deux</strong>
 * méthodes sondées, là où un pack de 272 mods en apporte 2 649. Il produisait donc du
 * travail, pas de la surface instrumentée — alors que les campagnes ont montré que le
 * coût de RUSTFORGE-X vient d'abord du <em>nombre</em> de méthodes portant une sonde
 * (ADR-021), et non de ce que ces sondes font.
 *
 * <p>Une unité générée est donc conçue pour deux choses à la fois : être assez longue
 * pour passer le seuil de sondage, et assez bon marché pour qu'en instancier des
 * milliers ne coûte presque rien en temps de tick.
 */
public interface SynthUnit {

    /**
     * Exécute l'unité.
     *
     * @param seed valeur d'entrée, qui empêche le compilateur de réduire le corps à une
     *     constante
     * @return la valeur calculée, à conserver par l'appelant
     */
    long run(long seed);

    /** @return l'identifiant de l'unité, unique dans le lot engendré */
    int unitId();
}
