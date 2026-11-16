# Autoridades (v0)

> Status: v0, tarefa 2.9: renomear um agente (2.9a), ligar uma autoridade por papel (2.9b) renomear um assunto (2.9c) e local de publicação e identificadores externos (2.9d). Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md), §16.3, e [edição](edicao.md). Critério da tarefa: **renomear um agente sobe o `row_version` das Instâncias ligadas e aparece no OPAC e no MARC exportado**.

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

### Ligar uma autoridade (2.9b)

6. O cabeçalho da coluna de Autoridades tem, para quem cataloga, o botão "+ Autoridade". Ele abre, abaixo da família, um painel (sem modal) para a Obra focada ou, sem uma Obra focada, para a primeira Obra da família; o título do painel diz qual (`GET /equipe/acervo/obra/{id}/autoridade/nova`; sem sessão, a página de entrada; sem `cataloguing`, 403).
7. O painel tem uma linha: a **classe** (pessoa, família, organização, evento, tópico, lugar, título, gênero/forma, período), o **nome**, o **papel** na Obra (contribuidor ou assunto), o **papel do contribuidor** (código de relator, como `ill`) e o **tesauro** do assunto (como `lcsh`). Ao digitar o nome, o painel sugere agentes já gravados quando o papel é contribuidor e assuntos quando é assunto; escolher uma sugestão preenche a classe (e o tesauro). Um nome sem sugestão igual é uma autoridade nova, marcada como tal.
8. Ligar manda `catalog.link_authority` (`POST /equipe/acervo/obra/{id}/autoridades`, com sessão e token CSRF). A regra do papel fica no comando, não no JavaScript: o tipo do comando só aceita agente como contribuidor (`bf:contribution`), e qualquer classe como assunto (`bf:subject`). O pedido é recusado (422, no campo) quando o nome está vazio ou passa de 2.000 caracteres, quando uma classe que não é agente vem como contribuidor, e quando a Obra já tem essa autoridade nesse papel (mesmo nome e classe; no contribuidor, o mesmo relator; no assunto, o mesmo tesauro). Obra que não existe, 404.
9. A autoridade é achada pela classe e pelo nome (e tesauro, no assunto) ou criada, e entra **depois** das outras contribuições ou assuntos da Obra. Um contribuidor ligado assim nunca é a entrada principal: ela se escolhe no formulário da Obra, porque muda o número de chamada e o `1XX` do MARC.
10. Na mesma transação, sobem a versão da Obra (um formulário aberto antes não grava a lista antiga de volta) e a de cada Instância dela, com o efeito de reindexação. A página do OPAC e o MARC exportado (`700`/`710`/`711`, `600`/`610`/`611`/`630`/`648`/`650`/`651`/`655`) mostram a ligação. O histórico da Obra lista a mudança como "Contribuições" ou "Assuntos". Depois de ligar, a tela recarrega.

### Assuntos na coluna (2.9c)

11. A coluna de Autoridades mostra também os assuntos que as Obras da família exibem (os três primeiros de cada Obra, como o cartão da Obra), uma vez cada, com o ícone da classe (tópico, lugar, período, gênero/forma; agente como assunto leva o ícone do agente) e a pílula "Assunto". Cada um liga à Obra por uma cordinha "sobre"; a do contribuidor principal continua "escrito por". O resumo da linha da família continua listando só os agentes.
12. O cartão de um assunto abre, para quem cataloga, o bloco do assunto (`GET /equipe/acervo/assunto/{id}/campos`), com o cabeçalho e o alcance (quantas Obras e registros a mudança alcança), como o do agente.
13. Salvar manda `catalog.update_subject` (`POST /equipe/acervo/assunto/{id}`), que segue as regras do item 3 com duas diferenças: o cabeçalho tomado é o de outro assunto da mesma classe, do mesmo tesauro e com as mesmas subdivisões (juntar é mesclagem); e um assunto com subdivisões tipadas (`madsrdf:ComplexSubject`) só aceita um cabeçalho com o mesmo número de partes depois de ` -- `, porque o tipo de cada subdivisão vale pela posição. Mudar as subdivisões é no formulário da Obra. A versão do assunto vem da migração 40.
14. O MARC exportado escreve o cabeçalho novo no `6XX` dos registros ligados; o OPAC e a busca o mostram depois do indexador.

### Local de publicação e identificadores externos (2.9d)

15. O painel "+ Autoridade" tem um terceiro papel, **local de publicação** (`bf:place` da `bf:provisionActivity`), que só aceita a classe lugar e vai para **uma Instância** da Obra: a primeira, ou a escolhida numa lista do painel quando a Obra tem mais de uma, pelo título, pela publicação e pelo ISBN do cartão (`?instancia=` no painel e no envio). Sem Instância, ou com outra classe, o pedido é recusado no campo; Instância de outra Obra, 404; o mesmo lugar que a Instância já tem, "já ligada". Grava o lugar da Instância, sobe a versão e reindexa só o registro dela; o histórico da Instância lista "Lugar". O MARC exportado escreve o lugar no `260`/`264 $a`. A página do OPAC ainda não mostra o lugar (só editora e data), como antes.
16. O bloco de um agente ou de um assunto mostra, abaixo do alcance, os **identificadores em outras bases** (`bf:identifiedBy`, `$0` e `$1` do MARC) que as Obras que o nomeiam trazem, cada um uma vez e até 50, como chips: um endereço web abre numa aba nova com o nome da base (LC, VIAF, Wikidata, ISNI, ORCID, GND, WorldCat) ou com o domínio; um número de controle aparece como está. Os identificadores continuam por contribuição e por assunto de cada Obra, como o MARC os traz; editá-los é no formulário da Obra.

## 2. Origem no Koha

- `C4::AuthoritiesMarc::merge` propaga a mudança de uma autoridade para os registros bibliográficos ligados pelo `$9`, em lote (`AuthorityMergeLimit`; acima dele, pelo cron `merge_authorities.pl`). Aqui o agente é a própria linha que os registros referenciam: renomear é uma linha, e o que se propaga é só a versão e a reindexação.

## 3. Paridade

`livre`: interface e modelo. Os rótulos do LCGFT (2.9e) estão na spec do BIBFRAME completo, item 12. Lugares como cadastro próprio (hoje o lugar da Instância é texto) ficam para depois da Fase 2.

Testes: `crates/elzevir-app/src/authorities.rs` (validação do renomear e do papel), `crates/elzevir/tests/opac.rs` (renomear agente e assunto e ligar na loja real: versões, posição, recusas, página do OPAC, busca e MARC exportado) e `crates/elzevir-web/tests/staff_catalog.rs` (rotas, permissões, alcance, formulário e recusas).
