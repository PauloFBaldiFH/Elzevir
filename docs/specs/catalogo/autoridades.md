# Autoridades (v0)

> Status: v0, tarefa 2.9, primeira parte (2.9a): renomear um agente. Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md), §16.3, e [edição](edicao.md). Critério da tarefa: **renomear um agente sobe o `row_version` das Instâncias ligadas e aparece no OPAC e no MARC exportado**.

## 1. Comportamento

1. Um agente (`bf:Person`, `bf:Family`, `bf:Organization`, `bf:Meeting`) é um só para todos os registros que o nomeiam como contribuidor. Editá-lo muda todos eles de uma vez.
2. O cartão de autoridade da família focada abre, para quem cataloga, o bloco do agente (`GET /equipe/acervo/autoridade/{id}/campos`; sem sessão, a página de entrada; sem a permissão `cataloguing`, 403; agente que não existe, 404). O bloco tem o nome autorizado e diz, antes de salvar, quantas Obras e quantos registros (Instâncias dessas Obras) a mudança alcança.
3. Salvar manda `catalog.update_agent` (`POST /equipe/acervo/autoridade/{id}`, com a sessão e o token CSRF, como as outras edições) com a versão lida. O comando:
   - recusa versão velha (409, como as outras edições);
   - recusa nome vazio ou com mais de 2.000 caracteres;
   - recusa o nome de outro agente do mesmo tipo: juntar os dois é uma mesclagem (§7 da spec de interação), não um renomear;
   - na mesma transação, sobe a versão do agente, de cada Obra que o nomeia e de cada Instância dessas Obras, e grava o efeito de reindexação de cada Instância. Assim um formulário de Obra aberto antes do renomear é recusado ao salvar (em vez de gravar o nome antigo de volta), o cache do OPAC por versão mostra o nome novo e a busca o encontra depois do indexador.
4. A exportação MARC escreve o nome novo no `100`/`110`/`111`/`700`/`710`/`711` dos registros ligados, porque a contribuição mudou em relação ao registro de origem; o resto do registro sai como entrou.
5. O comando vai para o `audit_log` com o autor e a hora, como os demais. O histórico de uma Obra ou Instância não lista o renomear, porque ele não editou a Obra.

## 2. Origem no Koha

- `C4::AuthoritiesMarc::merge` propaga a mudança de uma autoridade para os registros bibliográficos ligados pelo `$9`, em lote (`AuthorityMergeLimit`; acima dele, pelo cron `merge_authorities.pl`). Aqui o agente é a própria linha que os registros referenciam: renomear é uma linha, e o que se propaga é só a versão e a reindexação.

## 3. Paridade

`livre`: interface e modelo. Ficam para as próximas partes da 2.9: criar autoridade e buscar pela coluna ("+ Autoridade"), ligar por papel (`catalog.link_authority`), assuntos e lugares (`catalog.update_subject`), identificadores externos como chips e os rótulos do LCGFT.

Testes: `crates/elzevir-app/src/authorities.rs` (validação), `crates/elzevir/tests/opac.rs` (renomear na loja real: versões, recusas, página do OPAC, busca e MARC exportado) e `crates/elzevir-web/tests/staff_catalog.rs` (rotas, permissões, alcance e recusas).
