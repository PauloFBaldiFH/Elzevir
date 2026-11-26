# Ações em lote na listagem

> Status: tarefa 2.17, partes a (seleção e exportação), b (alterar exemplares) e c (listas com nome). Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 2, linha 2.17), [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §20 itens 8 e 11, [listagem](listagem.md) e [facetas](facetas.md).

## 1. Comportamento

### Seleção (parte a)

1. Cada linha-família da listagem tem uma caixa de seleção antes do ícone. Clicar nela marca ou desmarca a família sem abrir a linha.
2. A seleção é da aba do navegador: vale entre páginas, buscas, filtros e listas de trabalho, e some ao fechar a aba (`sessionStorage`). Linhas que chegam pelo carregamento da página seguinte já vêm marcadas se estavam na seleção.
3. Com ao menos uma família marcada, aparece abaixo da faixa uma barra com o número de famílias selecionadas, as ações e "Limpar seleção". Sem seleção, a barra fica escondida.
4. Valem até 500 famílias por seleção. A caixa de quem passaria disso não marca, e a barra avisa do limite.

### Exportar a seleção (parte a)

5. A barra exporta a seleção em MARC21 (ISO 2709), MARCXML ou UNIMARC ([UNIMARC](unimarc.md), item 16), para qualquer pessoa da equipe, como a exportação de um registro ([listagem](listagem.md), itens 33 a 36).
6. `GET /equipe/acervo/lote/marc?format=marc21|marcxml|unimarc|unimarcxml&head=h12&head=w7` exige sessão da equipe. `head` é a família como a listagem a escreve: `h` e o número da Matriz, ou `w` e o número da Obra. Família repetida conta uma vez; malformada é ignorada; passando de 500, as que sobram são ignoradas. Sem nenhuma família válida, ou com formato desconhecido, é 400.
7. Saem todas as Instâncias de cada família, na ordem da seleção, e dentro da família na ordem da listagem (Obra, depois Instância). Cada registro sai pela mesma regra da exportação de um registro e da linha de comando. O MARCXML é uma coleção (`<collection>`), como a exportação em lote do Koha.
8. O arquivo vem como anexo, `elzevir-lote.mrc` (`application/marc`) ou `elzevir-lote.xml` (`application/marcxml+xml`). Registro que não pode ser escrito fica de fora, e o cabeçalho `X-Elzevir-Not-Exported` lista as Instâncias que ficaram. Família que deixou de existir é ignorada.

### Alterar exemplares (parte b)

9. Para quem pode catalogar, a barra tem "Alterar exemplares". Abre uma janela com cinco campos: unidade de origem, unidade atual, política de circulação, localização e coleção. Cada um começa em "Não mudar"; localização e coleção têm também "Nenhuma", que as tira. Os locais e coleções aparecem com o nome da unidade a que pertencem.
10. Aplicar manda um comando só, `catalog.change_items { instances, set }`, com todas as Instâncias das famílias selecionadas e as mudanças escolhidas. O comando muda **todos os exemplares** dessas Instâncias, numa transação: ou todos mudam, ou nenhum.
11. Cada exemplar é conferido com o resultado das mudanças, pela mesma regra da grade ([exemplares](exemplares.md), item 5): unidade e política têm de existir, e localização e coleção têm de ser da unidade de origem do exemplar depois da mudança. Mudar a unidade de origem sem mudar uma localização da unidade antiga é recusado naquele exemplar. A recusa volta **por exemplar**: o código de barras (ou o tombo, ou o número), o campo e o motivo, todas de uma vez.
12. Recusas do lote inteiro: nenhuma mudança escolhida; nenhuma Instância; Instância que não existe; mais de 5.000 exemplares no lote.
13. Cada Instância alterada sobe de versão e vai para a reindexação, como na grade. O comando fica no `audit_log` com quem o fez, e o histórico de cada Instância mostra "Exemplares alterados em lote".
14. `POST /equipe/acervo/lote/exemplares` recebe `{"heads": [...], "set": {...}}` em JSON, com sessão, permissão de catalogar e o token CSRF, como as outras edições ([edição](edicao.md), item 17). Responde `{"instances": n, "items": m}`, ou 422 com as recusas. Depois de aplicar, a página recarrega e a seleção continua.

### Listas com nome (parte c)

15. Uma lista com nome guarda famílias para voltar a elas depois: "Doações de 2026", "Para revisar". As listas são da equipe: todos veem todas, e quem pode catalogar cria, acrescenta, tira e apaga.
16. Na barra da seleção, quem pode catalogar escolhe uma lista existente ou "Nova lista…" (com o nome) e manda a seleção para ela. Depois disso a página vai para a lista. Família que já está na lista não se repete.
17. O nome tem de 1 a 100 caracteres, sem espaços nas pontas, e não repete o de outra lista, sem distinguir maiúsculas. Uma lista guarda até 5.000 famílias; passar disso recusa a mudança inteira.
18. A faixa da listagem tem, depois dos botões das listas de trabalho ([facetas](facetas.md), item 13), o menu "Listas salvas" com o nome e o número de famílias de cada uma, em ordem de nome. A lista aberta fica marcada e dá nome ao menu.
19. Cada lista tem endereço próprio, `/equipe/acervo?list=s` e o número da lista, e se comporta como as outras listas de trabalho (facetas.md, itens 14 e 17): sem texto nem filtros, 30 famílias por página, família que deixou de existir sai. A família acrescentada por último vem primeiro.
20. Numa lista salva, a barra da seleção tem "Tirar desta lista", e a faixa tem "Apagar lista", que pede um segundo clique. Apagar tira só a lista; as famílias ficam no acervo.
21. Um comando só, `catalog.change_saved_list`, com quatro mudanças: criar com famílias, acrescentar, tirar e apagar. Fica no `audit_log` com quem o fez, mas não entra no histórico de nenhuma Instância nem em "Meus recentes": não muda o registro.
22. `POST /equipe/acervo/listas` recebe em JSON `{"action": "create", "name": ..., "heads": [...]}`, `{"action": "add" | "remove", "list": n, "heads": [...]}` ou `{"action": "delete", "list": n}`, com sessão, permissão de catalogar e o token CSRF. Responde `{"list": n, "url": ...}`; 422 com o campo `name` e o motivo quando o nome é recusado ou a lista passaria do limite; 404 para lista que não existe.

## 2. Origem no Koha

A alteração em lote de exemplares do Koha (`tools/batchMod.pl`, `Koha::Items::batch_update`) recebe códigos de barras ou números de exemplar, mostra um formulário com os subcampos do `952` e aplica um a um, pulando os que falham. No Elzevir a alteração parte das famílias selecionadas, muda só unidade, política, localização e coleção nesta parte, e é tudo ou nada.

O Koha junta registros no carrinho (`opac-basket.pl`, `basket/basket.pl`) e nas listas (`virtualshelves/shelves.pl`) e exporta por `basket/downloadcart.pl` e `virtualshelves/downloadshelf.pl`, com `Koha::Exporter::Record::export` (MARC21, MARCXML e outros). No Elzevir a seleção é de famílias, sem carrinho guardado no servidor. As listas do Koha (`Koha::Virtualshelves`) são privadas ou públicas e guardam registros; as do Elzevir são da equipe, guardam famílias e não aparecem no OPAC.

## 3. Paridade

`livre` (interface). Prova:

- `elzevir-app` (unidade): seleção lida dos parâmetros, sem repetição, com limite; Instâncias de cada família na ordem; MARCXML de vários registros numa coleção.
- `crates/elzevir-web/tests/staff_catalog.rs`: caixa em cada linha, barra escondida, exportação exige sessão, formato e seleção vazia dão 400.
- `elzevir-store` (unidade): 500 exemplares mudam numa transação; uma localização de outra unidade recusa o lote inteiro e diz quais exemplares; versão e reindexação das Instâncias; `audit_log` com as Instâncias tocadas.
- `crates/elzevir-web/tests/staff_catalog.rs`: alteração exige catalogar e CSRF; recusas voltam por exemplar.
- `crates/elzevir/tests/batch.rs`: a exportação da seleção sobre a base de testes dá os mesmos bytes que a exportação de cada registro, em sequência.
- `elzevir-store` (unidade): criar, acrescentar sem repetir, tirar e apagar; nome repetido sem distinguir maiúsculas e limite de famílias recusados; a lista lida na ordem do acréscimo; o comando não entra em "Meus recentes".
- `crates/elzevir-web/tests/staff_catalog.rs`: menu "Listas salvas" na faixa, página da lista, mudanças exigem catalogar e CSRF, nome recusado volta com o campo.
