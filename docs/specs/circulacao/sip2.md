# Servidor SIP2 (v0)

> Status: v0, tarefa 4.5. Base: 3M Standard Interchange Protocol 2.00, [empréstimo](emprestimo.md), [devolução](devolucao.md), [renovação](renovacao.md), [leitores](leitores.md) e [conta](conta.md). Critério da tarefa: **cenários SIP2 com as mesmas respostas do oráculo com o SIP ligado**.

Terminais de autoatendimento, cabines de devolução e catracas falam SIP2 com o sistema da biblioteca. O Elzevir responde como o servidor SIP do Koha (`C4::SIP`), sobre as mesmas regras do balcão: o SIP2 é só um codec que traduz mensagens em comandos de circulação e recusas em respostas (doc 03, §1.2, item 11).

A tarefa tem duas partes, cada uma num PR:

- **4.5a, codec:** quadros, checksum, leitura das mensagens pedidas e escrita das respostas, sem rede (itens 1 a 15).
- **4.5b, servidor:** a conexão, as contas de terminal, o login e cada mensagem ligada à circulação, com os cenários do oráculo (itens 16 em diante).

## 1. Comportamento

### Quadros (4.5a)

1. Uma mensagem termina em CR (`\r`). Antes de ler, a mensagem é limpa como no Koha (`C4::SIP::SIPServer::read_request`): saem do começo os brancos e os caracteres que não são letra, dígito ou `[\]^_\``; saem do começo e do fim os caracteres de controle e brancos; saem os LF e CRLF do meio. Uma mensagem vazia depois da limpeza é respondida com `96`.
2. Uma mensagem tem no máximo 8 KiB antes do CR. Acima disso a conexão é fechada sem resposta (o Koha não tem limite; o Elzevir põe um, como em toda entrada externa).
3. Os bytes são lidos como UTF-8; um byte inválido vira U+FFFD no valor do campo, sem recusar a mensagem.
4. A resposta termina em CRLF, ou em CR com `terminator = "cr"` na conta do terminal (padrão do Koha: CRLF). As respostas fixas `96` (pedido de reenvio) e o reenvio literal da última resposta terminam sempre em CR, como no Koha.

### Detecção de erro (4.5a)

5. O checksum é o complemento de dois, em 16 bits, da soma dos bytes da mensagem até o `AZ` inclusive, escrito com quatro dígitos hexadecimais maiúsculos (`C4::SIP::Sip::Checksum`). Ex.: `12345` dá 65281 (`FF01`).
6. Uma mensagem com mais de 11 bytes cujo nono byte a partir do fim começa `AY` liga a detecção de erro na sessão: o número de sequência é o byte depois de `AY`, e o checksum depois de `AZ` é conferido. Checksum errado é respondido com `96AZFEF6` e a mensagem não é tratada. `97AZFEF5` também liga a detecção e pede o reenvio.
7. Com a detecção ligada, cada resposta ganha `AY<sequência>AZ<checksum>` no fim, com a sequência do pedido (0 quando o pedido não tinha). Uma mensagem sem `AY` com a detecção ligada desliga a detecção.
8. **Reenvio** (`97`): sem resposta anterior, responde `96`; com uma resposta anterior sem `AY`, repete-a literalmente; com `AY`, repete-a com a sequência e o checksum novos.

### Mensagens pedidas (4.5a)

9. O código da mensagem são os dois primeiros bytes. Os campos fixos seguem o modelo do Koha (`C4::SIP::Sip::MsgType`, `%handlers`), cortados pela largura de cada um, com os brancos do fim tirados (como o `unpack "A"` do Perl); uma mensagem mais curta que o modelo tem os campos que couberem e nenhum campo variável.
10. Depois dos campos fixos vêm os campos variáveis, separados por `|`, cada um com dois caracteres de código e o valor. Um código que a mensagem não conhece é ignorado; um código repetido mantém o primeiro valor.
11. O Elzevir lê estas mensagens:

    | Código | Mensagem | Campos fixos | Campos variáveis |
    |---|---|---|---|
    | `93` | Login | algoritmo do usuário (1), da senha (1) | `CN`, `CO`, `CP` |
    | `99` | Estado do terminal | estado (1), largura de impressão (3), versão do protocolo (4) | |
    | `97` | Reenvio | | |
    | `23` | Estado do leitor | idioma (3), data (18) | `AO`, `AA`, `AC`, `AD` |
    | `63` | Informações do leitor | idioma (3), data (18), resumo (10) | `AO`, `AA`, `AC`, `AD`, `BP`, `BQ` |
    | `17` | Informações do exemplar | data (18) | `AO`, `AB`, `AC` |
    | `11` | Empréstimo | renovação pelo terminal (1), sem bloqueio (1), data (18), vencimento sem bloqueio (18) | `AO`, `AA`, `AB`, `AC`, `CH`, `AD`, `BO`, `BI` |
    | `09` | Devolução | sem bloqueio (1), data (18), data de devolução (18) | `AP`, `AO`, `AB`, `AC`, `CH`, `BI` |
    | `29` | Renovação | terceiro (1), sem bloqueio (1), data (18), vencimento sem bloqueio (18) | `AO`, `AA`, `AD`, `AB`, `AJ`, `AC`, `CH`, `BO` |
    | `35` | Fim da sessão do leitor | data (18) | `AO`, `AA`, `AC`, `AD` |

12. **Versão do protocolo**: a sessão começa na 1.00. `93` passa para a 2.00; `99` passa para a versão do campo (`1.` dá 1.00, `2.` dá 2.00, outra dá 1.00). `93` é lido em qualquer versão (ele mesmo passa para a 2.00). Na 1.00, as outras mensagens só da 2.00 (`63`, `17`, `29`, `35`) não são lidas, e `11` e `09` leem só os campos da 1.00 (`AO`, `AA`, `AB`, `AC` e `AP`, `AO`, `AB`, `AC`).
13. Qualquer outra mensagem, ou uma mensagem fora da versão, é respondida com `96`. **Desvio**: o Koha também trata bloqueio do leitor (`01`), reserva (`15`), atualização do exemplar (`19`), habilitação do leitor (`25`), pagamento (`37`) e renovar tudo (`65`); o Elzevir v0 responde `96` a elas e não as anuncia no `BX` ([desvios conhecidos](../../desvios-conhecidos.md)).

### Respostas (4.5a)

14. Cada resposta é escrita na ordem de campos do Koha (`handle_*` de `C4::SIP::Sip::MsgType`):
    - um campo obrigatório sai mesmo vazio (`AF|`); um campo opcional vazio não sai;
    - CR, LF e CRLF dentro de um valor viram um espaço, e o `|` vira `&#124;`;
    - datas e horas saem como `AAAAMMDD    HHMMSS` (quatro espaços), no fuso da biblioteca;
    - o vencimento sai com a hora do empréstimo, cortada no minuto (`C4::SIP::ILS::Item`); a data em que uma reserva deixa de esperar (`CM`) sai pelo dia, sempre com `235900` (`C4::SIP::Sip::timestamp` de uma data sem hora);
    - valores sim/não saem `Y`/`N`; as bandeiras de "negado" saem `Y` quando negado e branco quando não;
    - contagens saem com quatro dígitos (`0003`), e o tempo limite e as tentativas do `98` com três.
15. As respostas escritas são `94`, `98`, `24`, `64`, `18`, `12`, `10`, `30`, `36` e `96`, cada uma com os campos que o servidor da 4.5b preenche (itens 16 em diante).

### Configuração e conexão (4.5b)

16. O servidor é ligado pela seção `[sip2]` do `elzevir.toml`; sem ela, nenhuma porta abre:
    - `listen`: endereço e porta (o Koha usa 6001);
    - `timeout_seconds`: inatividade que fecha a conexão (padrão 300, o `client_timeout` do Koha);
    - `currency`: a moeda do `BH` (até três letras; sem ela, o campo não sai);
    - `[[sip2.terminals]]`, uma por terminal: `login` (uma conta da equipe), `unit` (o nome da unidade, que é o balcão das transações e o `AO` das respostas) e `terminator` (`crlf`, padrão, ou `cr`).
17. Até 64 conexões ao mesmo tempo; a seguinte é fechada sem resposta.
18. **Login** (`93`): a primeira mensagem da conexão tem de ser `93`; outra fecha a conexão sem resposta (`raw_transport` do Koha). `CN` e `CO` são o login e a senha de uma conta da equipe com a permissão `circulation`, e o login tem de estar em `[[sip2.terminals]]`. Recusado (ou com algoritmo de cifra diferente de `0`), responde `940` e espera outro `93`.
19. **Estado do terminal** (`99`): responde `98` com a política do exemplo do Koha (`SIPconfig.xml`): devolução, empréstimo e renovação sim, atualização de exemplar e modo offline não, tempo limite `100` e tentativas `005`, a hora, a versão e, na 2.00, o `BX` do item 13.

### Leitor (4.5b)

20. O leitor é achado pelo cartão (`AA`). Para o terminal, o leitor está **bloqueado** quando tem suspensão, cadastro vencido, ou deve mais que o limite de débito do balcão (`noissuescharge`, padrão 5,00); bloqueado, as quatro bandeiras de "negado" (empréstimo, renovação, recall e reserva) saem `Y`. Deve mais que o limite: as bandeiras de multas e taxas excessivas (posições 11 e 12) também. As outras bandeiras saem em branco (o Elzevir não tem cartão perdido, devolução contestada nem cobrança).
21. O nome é `<nome> <sobrenome>`. A mensagem de tela segue `C4::SIP::ILS::Patron::new`: `Greetings from Elzevir. `, mais `Your account has expired as of <data>` com o cadastro vencido, ` -- Patron blocked by fines` acima do limite e ` -- Patron owes <valor>` com débito (ou ` -- Patron has credit of <valor>` com crédito). A senha do leitor (`AD`) é a senha do OPAC: errada, a mensagem ganha ` -- Invalid password`.
22. Valores em dinheiro (`BV`, `CC` e as mensagens) saem como o Perl escreve um número: sem zeros à direita (`12.5`, `5`, `0`). `BV` sai sempre, mesmo zero; `CC` é o limite de débito.
23. **Informações do leitor** (`64`): reservas (todas), empréstimos atrasados, empréstimos, multas por exemplar (0), recalls (0) e reservas ainda não à espera; a lista pedida sai pelos códigos de barras, cortada por `BP` e `BQ` (de 1) como no `x_items` do Koha. Saem também `BD` (endereço), `BE`, `BF`, `PB` (nascimento `AAAAMMDD`), `PC` (categoria) e `PI` (`Y` só quando o leitor não está bloqueado de jeito nenhum: suspensão, cadastro vencido ou dívida acima do limite, porque no Koha o `noissues` da dívida também desliga o `inet`).
24. **Fim da sessão** (`35`): responde `36` com `Y` e `Thank you!`.

### Exemplar e circulação (4.5b)

25. **Informações do exemplar** (`17`), pelo código de barras: estado `10` em trânsito, `12` perdido, `04` emprestado, `08` separado para uma reserva, `01` danificado, `02` com "não emprestável" negativo, `03` disponível; marcador de segurança `02` e tipo de taxa `01` fixos, como no Koha; `AQ`, `AP` e `BG` com os nomes das unidades da casa, atual e da casa; `CF` com as reservas do registro; `AH` com o vencimento; `CM` com o último dia de uma reserva que espera por ele.
26. **Empréstimo** (`11`), na ordem do `C4::SIP::ILS::checkout`, no balcão da unidade do terminal:
    1. cartão desconhecido: `Invalid Patron`; código de barras desconhecido: `Invalid Item`;
    2. leitor bloqueado: `Patron debarred` (suspensão), `Patron expired on <data>`, `Patron has fines` ou `Patron blocked`;
    3. emprestado a outro leitor: `Item checked out to another patron`;
    4. recusa do balcão: `Issue failed : <motivo>`, com o nome do motivo no Koha (`TOO_MANY`, `NOT_FOR_LOAN`, `WTHDRAWN`, `RESTRICTED`, `DEBT`, `EXPIRED`, `DEBARRED`);
    5. confirmação pedida: já emprestado ao mesmo leitor renova, com `Item already checked out to you: renewing item.` e a renovação `Y`; reserva de outro leitor: `Item is reserved for another patron upon return.` ou, separado ou em trânsito para ela, `Item is on hold for another patron.`; débito: `Outstanding Fines block issue`; outra: `Item cannot be issued: <motivo>`.
    O empréstimo feito responde com o vencimento, `desensitize` `Y` e mídia magnética `N`.
27. **Devolução** (`09`), no balcão da unidade do terminal (`AP`, quando vem, não muda o balcão):
    - código de barras desconhecido: recusa com alerta `99` e `Invalid Item`; exemplar baixado: alerta `99` e `Item withdrawn, return not allowed`; não emprestado: recusa com `Item not checked out`;
    - devolvido: `AA` com o cartão de quem o tinha, `AQ` (unidade da casa), `AJ`, `CS`;
    - com reserva: o exemplar é separado para ela, como faz o Koha (`holds_get_captured`); retirada nesta unidade, alerta `01`; em outra, alerta `02` (vai em trânsito); `CY` e `DA` (nome, com e-mail e telefone entre parênteses) de quem reservou e `CT` com a unidade de retirada;
    - sem reserva, mas a voltar para outra unidade: alerta `04` e `CT` com ela; quando a transferência fica para a equipe, `This item must still be transferred to <unidade> branch.`;
    - `resensitize` `Y` quando devolvido; o alerta sai `Y` quando houve recusa ou tipo de alerta.
28. **Renovação** (`29`): cartão desconhecido: `Invalid patron barcode.`; leitor bloqueado: `Renewals not allowed.`; exemplar fora dos empréstimos do leitor: `Item not checked out to <nome>`; recusa do balcão com o código do `CanBookBeRenewed`, traduzido como no Koha: `Item unavailable due to outstanding holds`, `Item has reached maximum renewals`, e `too_soon`, `overdue`, `onsite_checkout`, `too_much_owing` e `restriction` como vêm. Renovada: o vencimento novo, `CK` vazio e `desensitize` `N`.
29. Mensagens com "sem bloqueio" (`no_block`) são tratadas como as outras: o Elzevir não aceita transações offline.
30. Cada mensagem tratada vai para o log com o evento `sip2.message`: terminal, código e resultado, sem senhas, cartões ou códigos de barras.

**Desvios** ([desvios conhecidos](../../desvios-conhecidos.md)): a saudação diz `Elzevir`; as unidades saem pelo nome, porque o núcleo não tem códigos de unidade; não há taxas de empréstimo (`BV` no `12` e no `30`); as mensagens do item 13; as transações offline do item 29.

## 2. Origem no Koha

- Quadros e checksum: `C4::SIP::SIPServer::read_request`, `C4::SIP::Sip::write_msg`, `C4::SIP::Sip::Checksum`.
- Mensagens: `C4::SIP::Sip::MsgType` (`%handlers`, `_initialize`, `handle`, `handle_*`, `send_acs_status`, `patron_status_string`).
- Campos: `C4::SIP::Sip::Constants`; formato: `C4::SIP::Sip::add_field`, `maybe_add`, `timestamp`, `add_count`, `sipbool`, `boolspace`, `denied`.
- Testes do Koha: `t/SIP/Sip.t` (checksum, data, campos) e `t/db_dependent/SIP/Message.t`.
- Diferenças intencionais: limite de 8 KiB por mensagem; delimitador fixo `|` (o Koha deixa configurar); sem `hide_fields`, `allow_fields`, modelos de campo (`ae_field_template`) e campos próprios (`custom_patron_fields`) nesta tarefa; as mensagens do item 13.

## 3. Paridade

Exigida (doc 04, §2). O codec é testado com mensagens tiradas dos testes do Koha e do padrão; o servidor, com cenários gravados do oráculo com o SIP ligado, nunca a rede.

## 4. Fora desta tarefa

- SIP sobre TLS e o transporte telnet do Koha.
- As mensagens do item 13 e o pagamento de multas pelo terminal.
- Codificação de saída diferente de UTF-8 (`encoding` do Koha).
