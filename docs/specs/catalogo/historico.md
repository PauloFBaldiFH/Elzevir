# Histórico do bloco focado (v0)

> Status: v0, tarefa 2.14. Base: [edição](edicao.md), item 18 (o `audit_log` guarda quem editou), e [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md). Critério da tarefa: **cada edição de Obra, Instância e exemplar aparece com autor e hora; só quem cataloga vê**.

## 1. Comportamento

### Registro

1. Todo comando gravado no `audit_log` leva a hora em que foi aplicado, em UTC (`YYYY-MM-DDTHH:MM:SSZ`), dada pelo banco na mesma transação, ao lado do autor (`actor`) que já guardava.
2. Cada comando do catálogo diz quais Obras e Instâncias tocou, numa tabela própria (`audit_entities`), escrita na mesma transação; o comando e a tabela vêm de uma função pura da camada app (`history::touched`), a partir do comando e do que ele devolveu:
   - família criada (pela tela ou importada): a Obra e a Instância;
   - Instância nova de uma Obra: a Obra e a Instância;
   - registro importado ou com a descrição substituída por importação: a Instância;
   - edição de Obra: a Obra; edição de Instância: a Instância;
   - grade de exemplares salva e exemplar acrescentado: a Instância (o exemplar aparece no histórico da Instância);
   - declarações de uma entidade: essa entidade.
3. A migração 37 cria a coluna da hora e a tabela, e lê das edições já gravadas de Obra, de Instância e de exemplares as entidades que tocaram; a hora delas não foi guardada e aparece como "antes do histórico". Importações e criações anteriores à migração não entram no histórico.

### Tela

4. O formulário do bloco focado de Obra ou de Instância, que só quem cataloga recebe ([edição](edicao.md), item 25), termina com o botão **Histórico**, que mostra e esconde a lista; na primeira vez, a tela lê `GET /equipe/acervo/obra/{id}/historico` ou `.../instancia/{id}/historico` (sem sessão: 401; sem a permissão `cataloguing`: 403) e mostra as linhas, da mais nova para a mais antiga, até 100.
5. Cada linha diz quando (a hora em UTC vinda do servidor, mostrada pelo script no fuso e no formato de quem vê), quem (o login da conta; "importação ou linha de comando" quando não há conta) e o quê: "Criado", "Importado de arquivo", "Alterado:" com os rótulos do formulário das partes editadas, na ordem da edição (a propriedade BIBFRAME pelo rótulo da ontologia), "Descrição substituída", ou o número de exemplares salvos.
6. O que cada comando mudou sai do JSON do próprio comando no `audit_log`; um comando que esta versão não sabe descrever aparece como "Outra alteração", sem erro.

## 2. Origem no Koha

- `C4::Log::logaction` grava em `action_logs` (módulo `CATALOGUING`, ações `ADD`, `MODIFY`, `DELETE`) quando a preferência `CataloguingLog` está ligada; a tela é `tools/viewlog.pl`, com filtro por registro.
- Aqui o registro não depende de preferência: o `audit_log` já é a fonte de verdade das escritas (doc 05, §1), e o histórico só o lê.

## 3. Paridade

`livre`: interface. O histórico não mostra o valor anterior de cada parte, só quais partes mudaram; quem precisa do valor encontra-o no JSON do comando.

Testes: `crates/elzevir-app/src/history.rs` (partes e entidades de cada comando), `crates/elzevir-store/tests/editing.rs` (histórico de Obra e Instância depois de importar e editar) e `crates/elzevir-web/tests/staff_catalog.rs` (rota, permissões e formulário).
