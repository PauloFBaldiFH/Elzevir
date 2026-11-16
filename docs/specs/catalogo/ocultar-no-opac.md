# Ocultar uma Instância do OPAC (v0)

> Status: v0, tarefa 2.15. Base: [edição](edicao.md), [busca](busca.md) e [mapeamento MARC21](mapeamento-marc21.md). Critério da tarefa: **Instância oculta não aparece na busca nem na página do OPAC, continua na equipe e exporta o `942 $n`**.

## 1. Comportamento

1. Toda Instância tem a marca **oculta do OPAC**, desligada por padrão. Não é propriedade BIBFRAME: diz como a biblioteca mostra o registro, não o que ele descreve, e por isso não entra no RDF (como a coluna `biblio.opac_suppressed` do Koha, fora do MARC descritivo).
2. Na importação, `942 $n` com valor `1` (sem espaços nas pontas) liga a marca; `0`, outro valor ou a falta do subcampo a deixam desligada.
3. Na exportação, uma Instância cuja marca mudou desde a importação grava `942 $n 1` quando oculta; quando volta a aparecer, grava `942 $n 0` se o registro já tinha `$n`, e não cria o subcampo se não tinha. Registro sem edição sai como foi gravado (ida e volta sem perdas).
4. A migração 38 cria a coluna e liga a marca das Instâncias já gravadas cujo registro de origem tem `942 $n 1`.
5. O formulário da Instância tem a caixa **Oculta do OPAC** na seção de metadados administrativos; a mudança é a parte `OpacHidden` da edição, sobe o `row_version` da Instância e aparece no [histórico](historico.md). Os campos do bloco focado mostram "Oculta do OPAC" quando ligada.
6. No OPAC, a busca deixa de fora as Instâncias ocultas (o total também), e a página `/record/{id}` de uma Instância oculta responde 404, como a de uma inexistente. A busca da equipe e as telas da equipe continuam mostrando-as.

## 2. Origem no Koha

- `C4::Biblio::TransformMarcToKoha` lê `biblio.opac_suppressed` do `942 $n` (mapeamento padrão do framework).
- Com a preferência `OpacSuppression` ligada, a busca do OPAC filtra `suppress:false` e `opac-detail.pl` chama `C4::Output::redirect_if_opac_suppressed`, que manda para `errors/404.pl` (ou `opac-blocked.pl` com `OpacSuppressionRedirect`).

## 3. Paridade

`livre`: interface. Diferenças intencionais: a marca vale sempre, sem a preferência `OpacSuppression`; não há `OpacSuppressionRedirect` nem `OpacSuppressionByIPRange` (a página oculta é sempre 404).

Testes: `crates/elzevir-app/tests/marc_round_trip.rs` (`942 $n` nos dois sentidos), `crates/elzevir/tests/opac.rs` (busca e página do OPAC, busca da equipe e exportação) e `crates/elzevir-web/tests/staff_catalog.rs` (campo do formulário).
