use std::net::{TcpListener, UdpSocket};
use std::thread;

use simple_dns::rdata::{A, AAAA, CNAME};
use simple_dns::ResourceRecord;

use super::super::tests::wait_for;
use super::*;

/// A reply to `query` carrying `answers`, with `rcode`, truncated or not.
fn reply_to(
    query: &[u8],
    answers: &[(&str, RData<'static>)],
    rcode: RCODE,
    truncated: bool,
) -> Vec<u8> {
    let query = Packet::parse(query).expect("a query");
    let mut reply = Packet::new_reply(query.id());
    *reply.rcode_mut() = rcode;
    if truncated {
        reply.set_flags(PacketFlag::TRUNCATION);
    }
    reply.questions = query.questions.clone();
    for (owner, rdata) in answers {
        reply.answers.push(ResourceRecord::new(
            Name::new_unchecked(owner),
            CLASS::IN,
            60,
            rdata.clone(),
        ));
    }
    reply.build_bytes_vec().expect("the reply builds")
}

/// The type a query asks.
fn rtype_of(query: &[u8]) -> TYPE {
    match Packet::parse(query).expect("a query").questions[0].qtype {
        QTYPE::TYPE(rtype) => rtype,
        other => panic!("a stub asks one type, asked {other:?}"),
    }
}

fn a(octets: [u8; 4]) -> RData<'static> {
    RData::A(A {
        address: u32::from(Ipv4Addr::from(octets)),
    })
}

fn aaaa(ip: Ipv6Addr) -> RData<'static> {
    RData::AAAA(AAAA {
        address: u128::from(ip),
    })
}

/// A UDP server on loopback answering each query `answer` makes of it.
fn serve_udp(answer: impl Fn(&[u8]) -> Option<Vec<u8>> + Send + 'static) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("a loopback UDP socket");
    let addr = socket.local_addr().unwrap();
    thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok((read, peer)) = socket.recv_from(&mut buffer) {
            if let Some(reply) = answer(&buffer[..read]) {
                let _ = socket.send_to(&reply, peer);
            }
        }
    });
    addr
}

fn conf_of(nameservers: Vec<SocketAddr>) -> Conf {
    Conf {
        nameservers,
        timeout: Duration::from_millis(300),
        attempts: 1,
    }
}

#[test]
fn resolv_conf_reads_up_to_three_servers_and_caps_its_options() {
    let conf = Conf::parse(
        "# comment\nnameserver 10.0.0.1\nnameserver bogus\nnameserver ::1\nnameserver 10.0.0.2\n\
         nameserver 10.0.0.3\noptions ndots:2 timeout:99 attempts:9\n",
    );
    assert_eq!(
        conf,
        Conf {
            nameservers: vec![
                "10.0.0.1:53".parse().unwrap(),
                "[::1]:53".parse().unwrap(),
                "10.0.0.2:53".parse().unwrap(),
            ],
            timeout: Duration::from_secs(RES_MAXRETRANS),
            attempts: RES_MAXRETRY,
        },
        "resolv.conf must yield at most MAXNS servers it can read, its timeout and attempts \
         capped as res_init caps them"
    );
}

#[test]
fn a_resolv_conf_naming_no_server_asks_the_local_one() {
    assert_eq!(
        Conf::parse("search example.com\n"),
        Conf {
            nameservers: vec!["127.0.0.1:53".parse().unwrap()],
            timeout: Duration::from_secs(RES_TIMEOUT),
            attempts: RES_DFLRETRY,
        },
        "a resolv.conf naming no server must ask 127.0.0.1 with glibc's default timeout and \
         attempts, as res_init does"
    );
}

#[test]
fn hosts_answers_each_address_naming_the_name() {
    let hosts = "127.0.0.1 localhost\n# ::2 localhost\n::1 ip6-localhost LOCALHOST\n\
                 10.0.0.9 other\n127.0.0.1 localhost.localdomain localhost\n";
    assert_eq!(
        (
            hosts_answer_of(hosts, "localhost"),
            hosts_answer_of(hosts, "missing")
        ),
        (
            Some(vec![
                "127.0.0.1".parse().unwrap(),
                "::1".parse().unwrap(),
                "127.0.0.1".parse().unwrap()
            ]),
            None
        ),
        "/etc/hosts must answer the address of every line an alias names, in any case, in its \
         order, skipping comments, and answer nothing for a name it lacks"
    );
}

#[test]
fn a_reply_counts_only_for_its_own_question_and_the_names_it_leads_to() {
    let query = encode_query(7, "app.test", TYPE::A).unwrap();
    let reply = reply_to(
        &query,
        &[
            (
                "app.test",
                RData::CNAME(CNAME(Name::new_unchecked("edge.test"))),
            ),
            ("edge.test", a([93, 184, 216, 34])),
            ("victim.test", a([10, 0, 0, 1])),
        ],
        RCODE::NoError,
        false,
    );
    assert_eq!(
        (
            reply_of(&reply, 7, "APP.test", TYPE::A),
            reply_of(&reply, 8, "app.test", TYPE::A),
            reply_of(&reply, 7, "other.test", TYPE::A),
            reply_of(&reply, 7, "app.test", TYPE::AAAA),
        ),
        (
            Some(Reply::Answered(vec!["93.184.216.34".parse().unwrap()])),
            None,
            None,
            None
        ),
        "a reply must count only for the ID, name, and type it answers, and take only addresses \
         on the CNAME chain from the name asked"
    );
}

#[test]
fn a_reply_tells_a_missing_name_a_failing_server_and_a_truncation_apart() {
    let query = encode_query(1, "app.test", TYPE::A).unwrap();
    let with = |rcode, truncated| {
        reply_of(
            &reply_to(&query, &[], rcode, truncated),
            1,
            "app.test",
            TYPE::A,
        )
    };
    assert_eq!(
        (
            with(RCODE::NameError, false),
            with(RCODE::ServerFailure, false),
            with(RCODE::NoError, true)
        ),
        (
            Some(Reply::Answered(Vec::new())),
            Some(Reply::Failed),
            Some(Reply::Truncated)
        ),
        "NXDOMAIN must answer no address, SERVFAIL must fail the server, and TC must ask again"
    );
}

#[test]
fn a_server_answers_both_questions() {
    let server = serve_udp(|query| {
        let answer = match rtype_of(query) {
            TYPE::A => a([93, 184, 216, 34]),
            _ => aaaa("2606:2800::1".parse().unwrap()),
        };
        Some(reply_to(
            query,
            &[("app.test", answer)],
            RCODE::NoError,
            false,
        ))
    });
    assert_eq!(
        wait_for(ask(conf_of(vec![server]), "app.test".into())).map(|found| found.ok()),
        Some(Some(vec![
            "2606:2800::1".parse().unwrap(),
            "93.184.216.34".parse().unwrap()
        ])),
        "a name server's AAAA and A answers must both come back"
    );
}

#[test]
fn a_server_answering_one_question_gives_that_answer() {
    let answered = |failing: RCODE| {
        let server = serve_udp(move |query| match rtype_of(query) {
            TYPE::A => Some(reply_to(
                query,
                &[("app.test", a([93, 184, 216, 34]))],
                RCODE::NoError,
                false,
            )),
            _ if failing == RCODE::NoError => None,
            _ => Some(reply_to(query, &[], failing, false)),
        });
        wait_for(ask(conf_of(vec![server]), "app.test".into())).map(|found| found.ok())
    };
    let a_only = Some(Some(vec!["93.184.216.34".parse().unwrap()]));
    assert_eq!(
        (answered(RCODE::NoError), answered(RCODE::ServerFailure)),
        (a_only.clone(), a_only),
        "a server that answers A but drops or fails AAAA must still give the A answer, as \
         glibc's send_dg takes the one reply it has"
    );
}

#[test]
fn a_question_lost_once_is_asked_again_alone() {
    let dropped = std::sync::atomic::AtomicBool::new(false);
    let server = serve_udp(move |query| match rtype_of(query) {
        TYPE::A if !dropped.swap(true, std::sync::atomic::Ordering::SeqCst) => None,
        TYPE::A => Some(reply_to(
            query,
            &[("app.test", a([93, 184, 216, 34]))],
            RCODE::NoError,
            false,
        )),
        _ => Some(reply_to(query, &[], RCODE::NoError, false)),
    });
    assert_eq!(
        wait_for(ask(conf_of(vec![server]), "app.test".into())).map(|found| found.ok()),
        Some(Some(vec!["93.184.216.34".parse().unwrap()])),
        "a server whose A reply is lost once must be asked for A again alone, as glibc's \
         send_dg resends in single-request mode, and its answer taken"
    );
}

#[test]
fn a_silent_server_gives_way_to_the_next() {
    let silent = serve_udp(|_| None);
    let answering = serve_udp(|query| {
        let answer = match rtype_of(query) {
            TYPE::A => vec![("app.test", a([93, 184, 216, 34]))],
            _ => Vec::new(),
        };
        Some(reply_to(query, &answer, RCODE::NoError, false))
    });
    assert_eq!(
        wait_for(ask(conf_of(vec![silent, answering]), "app.test".into())).map(|found| found.ok()),
        Some(Some(vec!["93.184.216.34".parse().unwrap()])),
        "a server silent past the timeout must give way to the next one in resolv.conf"
    );
}

// @behavior NW-228
#[test]
fn no_server_answering_is_a_temporary_failure_and_an_unaskable_name_no_name() {
    let silent = serve_udp(|_| None);
    let long_label = format!("{}.test", "a".repeat(64));
    assert_eq!(
        (
            wait_for(ask(conf_of(vec![silent]), "app.test".into())),
            wait_for(ask(conf_of(vec![silent]), long_label)),
        ),
        (
            Some(Err(LookupError::Again)),
            Some(Err(LookupError::NoName))
        ),
        "a lookup no name server answers must answer temporary-resolver-failure, as glibc's \
         EAI_AGAIN, and one whose name no query can carry name-unresolvable"
    );
}

#[test]
fn a_truncated_reply_is_asked_again_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let port = listener.local_addr().unwrap().port();
    let server = UdpSocket::bind(("127.0.0.1", port)).expect("the same port over UDP");
    let addr = server.local_addr().unwrap();
    thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok((read, peer)) = server.recv_from(&mut buffer) {
            let truncated = rtype_of(&buffer[..read]) == TYPE::A;
            let _ = server.send_to(
                &reply_to(&buffer[..read], &[], RCODE::NoError, truncated),
                peer,
            );
        }
    });
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut length = [0u8; 2];
            stream.read_exact(&mut length).unwrap();
            let mut query = vec![0u8; usize::from(u16::from_be_bytes(length))];
            stream.read_exact(&mut query).unwrap();
            let reply = reply_to(
                &query,
                &[("app.test", a([93, 184, 216, 34]))],
                RCODE::NoError,
                false,
            );
            stream
                .write_all(&(reply.len() as u16).to_be_bytes())
                .unwrap();
            stream.write_all(&reply).unwrap();
        }
    });
    assert_eq!(
        wait_for(ask(conf_of(vec![addr]), "app.test".into())).map(|found| found.ok()),
        Some(Some(vec!["93.184.216.34".parse().unwrap()])),
        "a reply truncated over UDP must be asked again over TCP, and its answer taken"
    );
}

#[test]
fn a_cname_chain_is_followed_in_any_order_and_a_loop_ends() {
    let query = encode_query(7, "app.test", TYPE::A).unwrap();
    let cname = |target: &'static str| RData::CNAME(CNAME(Name::new_unchecked(target)));
    let reply = reply_to(
        &query,
        &[
            ("edge.test", a([93, 184, 216, 34])),
            ("hop.test", cname("edge.test")),
            ("app.test", cname("hop.test")),
            ("loop.test", cname("back.test")),
            ("back.test", cname("loop.test")),
        ],
        RCODE::NoError,
        false,
    );
    let looped = reply_to(
        &encode_query(8, "loop.test", TYPE::A).unwrap(),
        &[
            ("loop.test", cname("back.test")),
            ("back.test", cname("loop.test")),
        ],
        RCODE::NoError,
        false,
    );
    assert_eq!(
        (
            reply_of(&reply, 7, "app.test", TYPE::A),
            reply_of(&looped, 8, "loop.test", TYPE::A)
        ),
        (
            Some(Reply::Answered(vec!["93.184.216.34".parse().unwrap()])),
            Some(Reply::Answered(Vec::new()))
        ),
        "a reply's CNAME chain must be followed whatever order its records come in, and a chain \
         looping back must end with the addresses it reached"
    );
}

#[test]
fn a_server_down_the_list_waits_as_send_dg_shares_the_time() {
    let conf = Conf {
        nameservers: vec![
            "10.0.0.1:53".parse().unwrap(),
            "10.0.0.2:53".parse().unwrap(),
            "10.0.0.3:53".parse().unwrap(),
        ],
        timeout: Duration::from_secs(6),
        attempts: 1,
    };
    assert_eq!(
        [0, 1, 2].map(|place| conf.timeout_of(place)),
        [6, 4, 8].map(Duration::from_secs),
        "the first server must wait the whole timeout, and each after it the timeout doubled \
         for its place, shared among the servers, as glibc's send_dg gives it"
    );
}

#[test]
fn a_name_res_hnok_refuses_answers_no_name_unasked() {
    assert_eq!(
        (
            ["app_1.test", "a!b.test", "-app.test", "a..test"].map(is_host_name),
            wait_for(resolve("a!b.test.")),
        ),
        ([true, false, false, false], Some(Err(LookupError::NoName))),
        "a name of letters, digits, '-', and '_' in non-empty labels, not starting with '-', \
         must be asked, and any other answer name-unresolvable at once, as glibc's res_hnok \
         judges"
    );
}

#[test]
fn a_config_holding_a_byte_no_encoding_spells_still_reads() {
    let path = std::env::temp_dir().join(format!("kobako-hosts-{}", std::process::id()));
    std::fs::write(&path, b"# caf\xe9\n10.0.0.9 app.test\n").unwrap();
    let hosts = read_config(path.to_str().unwrap());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        hosts_answer_of(&hosts, "app.test"),
        Some(vec!["10.0.0.9".parse().unwrap()]),
        "/etc/hosts holding a byte no encoding spells must still answer its other lines, as \
         glibc reads it as bytes"
    );
}
