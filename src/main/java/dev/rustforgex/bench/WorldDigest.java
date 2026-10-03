package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.ResourceKey;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Map;

/**
 * C-36 : empreinte de l'état d'un carré de chunks, pour les tests de gameplay.
 *
 * <p>Cahier des charges : PARTIE 20.3.4, test G-03 — « génération de 2 000 chunks,
 * comparaison NBT avec référence ». Critère : égalité d'état entre une exécution sans
 * RUSTFORGE-X et une exécution avec. Maturité : {@code STABLE}.
 *
 * <h2>Une empreinte par composante, pas une seule</h2>
 *
 * <p>Chaque chunk rend quatre empreintes : blocs, biomes, entités de bloc, structures.
 * Un écart doit dire <strong>où</strong> il est. « Les chunks diffèrent » n'apprend
 * rien ; « les entités de bloc diffèrent, les blocs non » désigne déjà un coupable.
 *
 * <h2>Ce qui est lu, et ce qui ne l'est pas</h2>
 *
 * <p>L'état est lu <strong>vivant</strong>, dans le chunk chargé, et non dans sa forme
 * sérialisée : la palette d'une section dépend de l'ordre dans lequel les blocs y ont été
 * posés, et deux sections identiques peuvent l'écrire dans deux ordres différents.
 * Comparer la forme sérialisée signalerait des écarts qui n'en sont pas.
 *
 * <p>Sont exclus, délibérément :
 * <ul>
 *   <li><strong>les entités</strong> — elles bougent, et l'état comparé est celui du monde
 *       généré ;
 *   <li><strong>la lumière et les cartes de hauteur</strong> — ce sont des grandeurs
 *       dérivées des blocs, recalculées par le jeu. Si les blocs sont égaux, ce qui s'en
 *       déduit l'est ; si la lumière seule différait, ce serait un défaut de calendrier
 *       de l'éclairage, pas une divergence d'état ;
 *   <li><strong>les ticks planifiés</strong> — leur échéance est relative à l'heure du
 *       jeu ; leur effet, lui, se voit dans les blocs.
 * </ul>
 *
 * <h2>Identité des états de bloc</h2>
 *
 * <p>Un état de bloc est désigné par son identifiant de registre. Il est le même d'une
 * exécution à l'autre tant que la liste de mods ne change pas — ce qui est le cas d'une
 * comparaison avec et sans RUSTFORGE-X, puisque RUSTFORGE-X n'enregistre aucun bloc.
 *
 * <p>Lit des milliers de chunks : à n'appeler que depuis un test de gameplay, jamais
 * dans un tick ordinaire (INV-14).
 */
public final class WorldDigest {

    /** Base et multiplicateur de FNV-1a sur 64 bits. */
    private static final long FNV_OFFSET = 0xcbf29ce484222325L;

    private static final long FNV_PRIME = 0x100000001b3L;

    /** Empreinte des quatre composantes d'un chunk. */
    public record ChunkDigest(long blocks, long biomes, long blockEntities, long structures) {
    }

    private WorldDigest() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Calcule l'empreinte d'un chunk déjà chargé et complet.
     *
     * @param level monde du chunk
     * @param chunk chunk à l'état {@code FULL}
     * @return ses quatre empreintes
     */
    public static ChunkDigest digest(ServerLevel level, LevelChunk chunk) {
        return new ChunkDigest(
                blocks(chunk), biomes(chunk), blockEntities(chunk), structures(level, chunk));
    }

    /**
     * États de bloc, section par section, dans l'ordre des positions.
     *
     * <p>Une section vide est comptée comme telle plutôt que sautée : sans cela, un chunk
     * à une section d'air de plus ou de moins aurait la même empreinte que l'autre.
     */
    static long blocks(LevelChunk chunk) {
        long[] hash = {FNV_OFFSET};
        for (LevelChunkSection section : chunk.getSections()) {
            if (section.hasOnlyAir()) {
                hash[0] = mix(hash[0], -1);
                continue;
            }
            section.getStates().getAll(state -> hash[0] = mix(hash[0], Block.getId(state)));
        }
        return hash[0];
    }

    /** Biomes, section par section, dans l'ordre des positions. */
    static long biomes(LevelChunk chunk) {
        long[] hash = {FNV_OFFSET};
        for (LevelChunkSection section : chunk.getSections()) {
            section.getBiomes().getAll(holder -> hash[0] = mix(hash[0], biomeId(holder)));
        }
        return hash[0];
    }

    /**
     * Entités de bloc, triées par position, avec leur contenu sérialisé.
     *
     * <p>Le contenu entre par l'empreinte de son {@link CompoundTag}, qui ne dépend pas de
     * l'ordre d'insertion des clés : deux coffres au même contenu ont la même empreinte,
     * quel que soit l'ordre dans lequel le jeu a rempli leurs champs.
     */
    static long blockEntities(LevelChunk chunk) {
        List<Map.Entry<BlockPos, BlockEntity>> entries =
                new ArrayList<>(chunk.getBlockEntities().entrySet());
        Comparator<BlockPos> byPosition = Comparator
                .comparingInt((BlockPos pos) -> pos.getY())
                .thenComparingInt(pos -> pos.getZ())
                .thenComparingInt(pos -> pos.getX());
        entries.sort(Map.Entry.comparingByKey(byPosition));

        long hash = FNV_OFFSET;
        for (Map.Entry<BlockPos, BlockEntity> entry : entries) {
            BlockPos pos = entry.getKey();
            hash = mix(hash, pos.getX());
            hash = mix(hash, pos.getY());
            hash = mix(hash, pos.getZ());
            CompoundTag saved = entry.getValue().saveWithFullMetadata();
            hash = mix(hash, saved.hashCode());
        }
        return hash;
    }

    /**
     * Départs et références de structures.
     *
     * <p>C'est l'une des sorties majeures de la génération, et celle qui dépend le plus de
     * code de mods : un village, une ruine ou un donjon déplacé d'un chunk ne laisserait
     * aucune trace dans les blocs de son chunk d'origine.
     */
    static long structures(ServerLevel level, LevelChunk chunk) {
        Registry<Structure> registry = level.registryAccess().registryOrThrow(Registries.STRUCTURE);

        List<String> starts = new ArrayList<>();
        for (Map.Entry<Structure, StructureStart> entry : chunk.getAllStarts().entrySet()) {
            StructureStart start = entry.getValue();
            if (!start.isValid()) {
                continue;
            }
            BoundingBox box = start.getBoundingBox();
            starts.add(keyOf(registry, entry.getKey()) + '@' + box.minX() + ',' + box.minY()
                    + ',' + box.minZ() + ':' + box.maxX() + ',' + box.maxY() + ',' + box.maxZ()
                    + '#' + start.getPieces().size());
        }
        starts.sort(Comparator.naturalOrder());

        List<String> references = new ArrayList<>();
        chunk.getAllReferences().forEach((structure, origins) -> {
            long[] sorted = origins.toLongArray();
            Arrays.sort(sorted);
            references.add(keyOf(registry, structure) + Arrays.toString(sorted));
        });
        references.sort(Comparator.naturalOrder());

        long hash = FNV_OFFSET;
        for (String start : starts) {
            hash = mix(hash, start.hashCode());
        }
        hash = mix(hash, -1);
        for (String reference : references) {
            hash = mix(hash, reference.hashCode());
        }
        return hash;
    }

    private static String keyOf(Registry<Structure> registry, Structure structure) {
        ResourceLocation key = registry.getKey(structure);
        return key == null ? "?" : key.toString();
    }

    private static int biomeId(Holder<Biome> holder) {
        return holder.unwrapKey()
                .map(ResourceKey::location)
                .map(ResourceLocation::toString)
                .map(String::hashCode)
                .orElse(0);
    }

    /** Une étape de FNV-1a, sur les quatre octets d'un entier. */
    static long mix(long hash, int value) {
        long h = hash;
        for (int shift = 0; shift < 32; shift += 8) {
            h ^= (value >>> shift) & 0xff;
            h *= FNV_PRIME;
        }
        return h;
    }
}
