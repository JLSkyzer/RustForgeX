package rfxbench.synth;

import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.fml.common.Mod;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.List;
import java.util.function.Consumer;

/**
 * Mod synthétique de charge (PARTIE 22, R-870, R-871).
 *
 * <p>Ce n'est pas un mod de contenu et il n'imite personne. Il décrit une <strong>forme
 * de charge</strong> — coût CPU par tick, taux d'allocation, gestionnaires d'événements,
 * fils propres, non-déterminisme — que le cahier des charges demande de savoir produire
 * sans dépendre d'un mod tiers.
 *
 * <h2>Pourquoi un artefact séparé</h2>
 *
 * <p>Il vit dans son propre paquet, {@code rfxbench.synth}, et dans son propre JAR.
 * C'est indispensable : {@code TargetScanner} refuse d'instrumenter {@code dev/rustforgex/},
 * donc une charge synthétique placée dans le JAR du mod ne serait jamais sondée — et ne
 * mesurerait rien de ce qu'elle est censée mesurer.
 *
 * <h2>Inerte par défaut</h2>
 *
 * <p>Sans les propriétés de {@link SynthConfig}, ce mod se charge, ne s'abonne à rien,
 * ne crée aucun fil et ne consomme rien. Il peut donc rester dans {@code mods} entre
 * deux campagnes sans fausser la moindre mesure.
 *
 * <h2>Ce qu'il ne doit jamais faire</h2>
 *
 * <p>Il ne touche pas au monde. Une charge qui poserait des blocs ou déplacerait des
 * entités rendrait deux exécutions incomparables, et le harnais a déjà
 * {@code LoadProfile} pour cela.
 */
@Mod(SynthMod.MODID)
public final class SynthMod {

    /** Identifiant du mod synthétique. */
    public static final String MODID = "rfxbenchsynth";

    private static final Logger LOGGER = LoggerFactory.getLogger("rfxbench-synth");

    private final SynthConfig config = SynthConfig.fromSystem();
    private final List<SynthWorkload> workloads = new ArrayList<>();
    private final List<Thread> threads = new ArrayList<>();

    /** Consommé pour que le calcul ne soit pas éliminé par le compilateur. */
    private volatile long sink;

    /** Construit le mod et n'installe que ce qui a quelque chose à faire. */
    public SynthMod() {
        if (config.idle()) {
            LOGGER.info("Mod synthétique présent et inerte : aucune propriété {}* fournie.",
                    SynthConfig.PREFIX);
            return;
        }
        LOGGER.info("Mod synthétique armé : {}", config);

        for (int i = 0; i < config.workloads(); i++) {
            workloads.add(new SynthWorkload(i));
        }
        registerHandlers();
        startThreads();
    }

    /**
     * Enregistre les gestionnaires d'événements demandés, priorités mêlées.
     *
     * <p>Le premier fait le travail des unités ; les suivants ne font presque rien. Ce
     * qu'on cherche à reproduire ici, c'est le <strong>nombre</strong> de gestionnaires
     * et la variété de leurs priorités, qui est ce qu'un modpack chargé impose au bus.
     */
    private void registerHandlers() {
        EventPriority[] priorities = EventPriority.values();

        Consumer<TickEvent.ServerTickEvent> worker = this::runWorkloads;
        MinecraftForge.EVENT_BUS.addListener(
                EventPriority.NORMAL, false, TickEvent.ServerTickEvent.class, worker);

        for (int i = 0; i < config.handlers(); i++) {
            EventPriority priority = priorities[i % priorities.length];
            final int index = i;
            MinecraftForge.EVENT_BUS.addListener(priority, false,
                    TickEvent.ServerTickEvent.class, event -> observe(index, event));
        }
    }

    /** Gestionnaire de figuration : il coûte le passage, et rien de plus. */
    private void observe(int index, TickEvent.ServerTickEvent event) {
        if (event.phase == TickEvent.Phase.END) {
            sink += index;
        }
    }

    /** Exécute toutes les unités de travail, une fois par tick. */
    private void runWorkloads(TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END) {
            return;
        }
        long value = sink;
        for (SynthWorkload workload : workloads) {
            value += workload.tick(config.iterations(), config.allocationBytes(),
                    config.nondeterministic());
        }
        sink = value;
    }

    /**
     * Démarre les fils propres au mod synthétique (R-870).
     *
     * <p>Le cahier des charges exige un profil fortement multithreadé, pour vérifier que
     * RUSTFORGE-X le refuse ou le sérialise plutôt que de le casser. Ces fils ne touchent
     * jamais à l'état du jeu : ils calculent, et c'est tout.
     */
    private void startThreads() {
        for (int i = 0; i < config.threads(); i++) {
            SynthWorkload own = new SynthWorkload(10_000 + i);
            Thread thread = new Thread(() -> {
                while (!Thread.currentThread().isInterrupted()) {
                    sink += own.tick(config.iterations(), config.allocationBytes(),
                            config.nondeterministic());
                    try {
                        Thread.sleep(1L);
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                        return;
                    }
                }
            }, "RFXBench-Synth-" + i);
            // Démon : un mod de charge ne doit jamais retarder l'arrêt du serveur.
            thread.setDaemon(true);
            threads.add(thread);
            thread.start();
        }
    }

    /** @return les réglages en vigueur */
    public SynthConfig config() {
        return config;
    }

    /** @return le nombre d'unités de travail créées */
    public int workloadCount() {
        return workloads.size();
    }

    /** @return le nombre de fils démarrés */
    public int threadCount() {
        return threads.size();
    }

    /** Empêche l'élimination du calcul, et sert aux diagnostics. */
    public long sink() {
        return sink;
    }
}
