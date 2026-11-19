# Regras de circulação

> Status: tarefa 3.2, parte a (resolução, comando e gravação). A tela em grade vem na parte b. Base: Koha 26.05.05 e [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 3, linha 3.2; §3.4, "uma função de resolução que explica de onde veio cada valor").

## 1. Comportamento

1. Uma regra é um valor de um **tipo** (os 57 de `$RULE_KINDS`, já gerados em `elzevir-data`) num **escopo** de três dimensões: unidade, categoria de leitor e política de circulação. Cada dimensão é um registro dos cadastros ou "qualquer". A política ocupa o lugar do tipo de item do Koha, que o Elzevir não tem.
2. Há no máximo uma regra por tipo e escopo. O valor é texto de até 32 caracteres, e **texto vazio é um valor**: uma regra vazia mais específica esconde um número de uma regra menos específica.
3. **Resolução.** A regra que vale para uma unidade, uma categoria e uma política é escolhida entre as regras cujo escopo cobre o pedido. Uma dimensão da regra cobre o pedido quando é "qualquer" ou é igual à pedida. Quando o pedido diz "qualquer", só regras com "qualquer" nessa dimensão servem. Entre as que cobrem, ganha a que tem unidade; depois, a que tem categoria; depois, a que tem política. Por isso uma regra da unidade para qualquer leitor e qualquer política vence uma regra de qualquer unidade para a categoria e a política exatas.
4. A resolução devolve a própria regra, com seu escopo, que diz de onde veio o valor. É uma função pura do domínio (`effective_rule`), a única que resolve regras.
5. **Gravação**, comando `circ.set_rules`, com a permissão de administração (Koha: `manage_circ_rules`). O comando recebe um escopo e vários tipos com seus valores, e grava tudo ou nada:
   - um valor troca o anterior;
   - nenhum valor apaga a regra;
   - um valor vazio apaga a regra quando o tipo não pode ficar vazio (`can_be_blank` falso);
   - tipo desconhecido ou repetido é recusado;
   - escopo com uma dimensão que o tipo não usa é recusado (por exemplo, `max_holds` por política);
   - valor monetário que não é algarismos com parte decimal opcional (`^\d+(\.\d+)?$`) é recusado, assim como valor com mais de 32 caracteres;
   - unidade, categoria ou política inexistente no escopo é recusada.
6. Unidade, categoria de leitor e política usadas por alguma regra não podem ser apagadas.
7. As regras não entram no resumo da importação (não são conteúdo do catálogo).

## 2. Origem no Koha

- `Koha::CirculationRules::get_effective_rule`: busca com `branchcode`, `categorycode` e `itemtype` iguais ao pedido ou `NULL`, ordenada pelas três colunas de forma descendente, primeira linha. No MySQL, `NULL` fica por último numa ordem descendente.
- `Koha::CirculationRules::set_rule`: escopo exigido por tipo, vazio que vira remoção, conferência de valor decimal.
- `circulation_rules.rule_value` é `varchar(32)`.

## 3. Paridade

`exigida` na resolução. Prova:

- `scenarios/circulacao/regras-efetivas.koha.json`: as regras do oráculo e as efetivas que o Koha devolveu (`GET /api/v1/circulation_rules?effective=true`) para 24 combinações (unidade ou qualquer; duas categorias ou qualquer; três tipos de item ou qualquer). `crates/elzevir-scenarios/tests/rules.rs` resolve as mesmas regras no Elzevir: as 24 × 57 = 1.368 respostas são iguais. Os códigos do Koha viram ids genéricos dentro do teste e não chegam ao domínio.
- O gravador é `tools/circ-rules-recorder/record.py`. As regras reais do oráculo são poucas, então ele acrescenta seis conjuntos temporários (unidade contra categoria, categoria contra tipo, vazio que esconde número, regra geral), grava e os remove, conferindo que as regras voltaram a ser as de antes.
- Testes de unidade da resolução e dos valores; `crates/elzevir-store/tests/circulation_rules.rs`: gravar, trocar, apagar, vazio mantido, vazio que apaga, recusas sem gravar nada, política em uso.

## 4. Casos de borda

1. Regra vazia de unidade (`finedays` vazio em MAIN) vence `finedays = 2` de qualquer unidade para a categoria e o tipo exatos: o resultado é vazio.
2. Pedido com unidade "qualquer" ignora todas as regras de unidade.
3. Uma política nova, sem regra própria, recebe as regras de qualquer política.

## 5. Desvios

Nenhum na resolução. O Koha guarda o tipo de item no escopo; o Elzevir guarda a política de circulação, que é a mesma dimensão com outro nome (ADR 0003).
