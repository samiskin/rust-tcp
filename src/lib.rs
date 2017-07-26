pub mod utils;
pub mod tcp;
pub mod segment;
pub mod config;
use tcp::*;
use std::str;
use std::net::*;
use config::*;
use segment::*;
use utils::*;
use std::collections::HashMap;
use std::io::prelude::*;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{Sender, Receiver, RecvError, SendError};
use std::fs::{File, OpenOptions};
use std::path::Path;


fn tuple_to_filename(tuple: &TCPTuple) -> String {
    format!(
        "{}.{}.{}.{}",
        tuple.dst.ip(),
        tuple.dst.port(),
        tuple.src.ip(),
        tuple.src.port()
    )
}

fn get_file(tuple: &TCPTuple, folder: &Path) -> Result<File, std::io::Error> {
    let filepath = folder.join(tuple_to_filename(&tuple));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .append(true)
        .create(true)
        .open(filepath)?;

    Ok(file)
}

fn send_str(tcb_input: &Sender<TCBInput>, s: String) -> Result<(), SendError<TCBInput>> {
    let len: u32 = s.len() as u32;
    let mut bytes = u32_to_u8(len);
    bytes.extend(s.into_bytes());
    tcb_input.send(TCBInput::Send(bytes))?;
    Ok(())
}

fn recv_str(tcb_output: &Receiver<u8>) -> Result<String, RecvError> {
    let size = buf_to_u32(&TCB::recv(&tcb_output, 4)?[..]);
    Ok(String::from_utf8(TCB::recv(&tcb_output, size)?).unwrap())
}

fn run_server_tcb(config: Config, tuple: TCPTuple, input: Sender<TCBInput>, output: Receiver<u8>) {
    let mut file = if let Ok(file) = get_file(&tuple, config.filepath.as_path()) {
        file
    } else {
        input.send(TCBInput::Close).unwrap();
        return;
    };

    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    send_str(&input, s).unwrap_or_else(|_| return);

    'main_application_loop: loop {
        match recv_str(&output) {
            Ok(data) => {
                // println!("Got string: {:?}", data);
                file.write_all(&data.as_bytes()).unwrap();
                // Errors when Closed
                if send_str(&input, data).is_err() {
                    break 'main_application_loop;
                }
            }
            Err(_) => break 'main_application_loop,
        }
    }
    file.sync_all().unwrap();
    println!("Server TCB Ending");
}

fn multiplexed_receive(
    config: &Config,
    channels: &mut HashMap<TCPTuple, Sender<TCBInput>>,
    socket: &UdpSocket,
) -> Result<(), ()> {
    let mut buf = vec![0; (1 << 16) - 1];
    match socket.recv_from(&mut buf) {
        Ok((amt, src)) => {
            buf.truncate(amt);
            let seg = Segment::from_buf(buf.clone());
            if seg.validate() {
                let tuple = TCPTuple {
                    src: socket.local_addr().unwrap(),
                    dst: src, // Send replies to the sender
                };
                let mut valid_channel_found = false;
                match channels.entry(tuple) {
                    Entry::Occupied(entry) => {
                        let seg_copy = seg.clone();
                        match entry.into_mut().send(TCBInput::Receive(seg_copy)) {
                            Ok(_) => {
                                valid_channel_found = true;
                            }
                            Err(_) => {}
                        }
                    }
                    _ => {}
                }
                if !valid_channel_found {
                    channels.remove(&tuple);
                }
                match channels.entry(tuple) {
                    Entry::Vacant(v) => {
                        println!("New connection! {:?}", tuple);
                        let (mut tcb, input, output) = TCB::new(tuple, socket.try_clone().unwrap());
                        let udp_sender = input.clone();
                        udp_sender.send(TCBInput::Receive(seg)).unwrap();
                        v.insert(udp_sender);
                        let config = config.clone();
                        std::thread::spawn(move || tcb.run_tcp());
                        std::thread::spawn(
                            move || { run_server_tcb(config, tuple, input, output); },
                        );
                    }
                    _ => {}
                }
            }
        }
        Err(_) => return Err(()),
    };

    return Ok(());
}

pub fn run_server(config: Config) -> Result<(), ()> {
    println!("Starting Server...");

    let mut channels: HashMap<TCPTuple, Sender<TCBInput>> = HashMap::new();
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", config.port)).unwrap();

    'event_loop: loop {
        multiplexed_receive(&config, &mut channels, &socket)?;
    }
}

pub fn run_client(config: ClientConfig) -> Result<(), ()> {
    println!("Starting Client...");
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", config.src_port)).unwrap();
    let tuple = TCPTuple {
        src: socket.local_addr().unwrap(),
        dst: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.dst_port),
    };
    let (mut tcb, input, output) = TCB::new(tuple, socket.try_clone().unwrap());
    let tcb_thread = std::thread::spawn(move || tcb.run_tcp());
    input.send(TCBInput::SendSyn);

    let seg_input = input.clone();
    std::thread::spawn(move || 'socket_loop: loop {
        let mut buf = vec![0; (1 << 16) - 1];
        match socket.recv_from(&mut buf) {
            Ok((amt, src)) => {
                buf.truncate(amt);
                let seg = Segment::from_buf(buf);
                if seg.validate() {
                    seg_input.send(TCBInput::Receive(seg));
                }
            }
            Err(_) => {}
        }
    });

    let file_contents = recv_str(&output).unwrap();
    // println!("Current File Contents {}", file_contents);
    send_str(&input, String::from("\n lol cool story bro")).unwrap();

    let echo1 = recv_str(&output).unwrap();
    // println!("Echo 1 {}", echo1);

    input.send(TCBInput::Close);
    tcb_thread.join();

    println!("Ending Client");

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn get_file_test() {
        let tuple = TCPTuple {
            src: "127.0.0.1:54321".parse().unwrap(),
            dst: "127.0.0.1:12345".parse().unwrap(),
        };
        let folderpath = Path::new("./");
        let mut file = get_file(&tuple, &folderpath).unwrap();
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        println!("Got file of length {}", s.len());

        let filepath = folderpath.join(tuple_to_filename(&tuple));
        std::fs::remove_file(filepath).unwrap();
    }

    // const SCRIPT: &'static str = "Did you ever hear the tragedy of Darth Plagueis The Wise? I thought not. It’s not a story the Jedi would tell you. It’s a Sith legend. Darth Plagueis was a Dark Lord of the Sith, so powerful and so wise he could use the Force to influence the midichlorians to create life… He had such a knowledge of the dark side that he could even keep the ones he cared about from dying. The dark side of the Force is a pathway to many abilities some consider to be unnatural. He became so powerful… the only thing he was afraid of was losing his power, which eventually, of course, he did. Unfortunately, he taught his apprentice everything he knew, then his apprentice killed him in his sleep. Ironic. He could save others from death, but not himself.";

    #[test]
    fn transfer_data() {
        let ((server_input, server_output, _), (client_input, client_output, _)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        client_input.send(TCBInput::SendSyn).unwrap();

        send_str(&server_input, String::from(SCRIPT)).unwrap();
        let output = recv_str(&client_output).unwrap();
        assert_eq!(output, String::from(SCRIPT));

        send_str(&client_input, String::from(SCRIPT)).unwrap();
        let output = recv_str(&server_output).unwrap();
        assert_eq!(output, String::from(SCRIPT));
    }

    fn get_tuples_from_socks(
        server_sock: &UdpSocket,
        client_sock: &UdpSocket,
    ) -> (TCPTuple, TCPTuple) {
        let server_tuple = TCPTuple {
            src: server_sock.local_addr().unwrap(),
            dst: client_sock.local_addr().unwrap(),
        };
        let client_tuple = TCPTuple {
            src: client_sock.local_addr().unwrap(),
            dst: server_sock.local_addr().unwrap(),
        };
        (server_tuple, client_tuple)
    }

    #[test]
    fn file_echo_test() {
        let ((server_input, server_output, server_sock),
             (client_input, client_output, client_sock)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        let (server_tuple, _) = get_tuples_from_socks(&server_sock, &client_sock);

        let server_config = Config {
            port: server_sock.local_addr().unwrap().port(),
            filepath: PathBuf::from("./"),
        };

        let _server = std::thread::spawn(move || {
            run_server_tcb(server_config, server_tuple, server_input, server_output);
        });

        let filepath = Path::new("./");
        let filepath = filepath.join(tuple_to_filename(&server_tuple));
        let mut file = File::create(filepath.clone()).unwrap();
        let initial_contents = String::from(
            "Did you ever hear the tragedy of Darth Plagueis the wise?\n",
        );
        file.write_all(&initial_contents.as_bytes()).unwrap();
        file.flush().unwrap();
        file.sync_data().unwrap();
        drop(file);

        client_input.send(TCBInput::SendSyn).unwrap();
        let file_contents = recv_str(&client_output).unwrap();

        // NOTE: Sometimes the write doesn't actually succeed even though both flush and sync_data
        //       are called, so this assertion might fail...  just re-run the test if it does
        assert_eq!(
            file_contents,
            initial_contents,
            "\n\x1b[35m NOTE: This test may not have actually failed, try re-running \x1b[0m"
        );

        let response = String::from("It's not a story the jedi would tell you");
        send_str(&client_input, response.clone()).unwrap();
        let ack = recv_str(&client_output).unwrap();

        assert_eq!(ack, response);

        client_input.send(TCBInput::Close).unwrap();
        _server.join().unwrap();

        assert!(client_input.send(TCBInput::Close).is_err());

        std::fs::remove_file(filepath.clone()).unwrap();
    }

    #[test]
    #[ignore] // Reliant on existance of root_only_dir which is owned by root with permissions 700
    fn server_close_test() {
        let ((server_input, server_output, server_sock),
             (client_input, client_output, client_sock)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        let (server_tuple, _) = get_tuples_from_socks(&server_sock, &client_sock);

        let server_config = Config {
            port: server_sock.local_addr().unwrap().port(),
            filepath: PathBuf::from("./root_only_dir/"),
        };

        let _server = std::thread::spawn(move || {
            run_server_tcb(server_config, server_tuple, server_input, server_output);
        });

        client_input.send(TCBInput::SendSyn).unwrap();
        assert!(client_output.recv().is_err());
    }


    const SCRIPT: &'static str = "\
Call me Ishmael. Some years ago--never mind how long precisely--having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral principle to prevent me from deliberately stepping into the street, and methodically knocking people's hats off--then, I account it high time to get to sea as soon as I can. This is my substitute for pistol and ball. With a philosophical flourish Cato throws himself upon his sword; I quietly take to the ship. There is nothing surprising in this. If they but knew it, almost all men in their degree, some time or other, cherish very nearly the same feelings towards the ocean with me.

There now is your insular city of the Manhattoes, belted round by wharves as Indian isles by coral reefs--commerce surrounds it with her surf. Right and left, the streets take you waterward. Its extreme downtown is the battery, where that noble mole is washed by waves, and cooled by breezes, which a few hours previous were out of sight of land. Look at the crowds of water-gazers there.

Circumambulate the city of a dreamy Sabbath afternoon. Go from Corlears Hook to Coenties Slip, and from thence, by Whitehall, northward. What do you see?--Posted like silent sentinels all around the town, stand thousands upon thousands of mortal men fixed in ocean reveries. Some leaning against the spiles; some seated upon the pier-heads; some looking over the bulwarks of ships from China; some high aloft in the rigging, as if striving to get a still better seaward peep. But these are all landsmen; of week days pent up in lath and plaster--tied to counters, nailed to benches, clinched to desks. How then is this? Are the green fields gone? What do they here?

But look! here come more crowds, pacing straight for the water, and seemingly bound for a dive. Strange! Nothing will content them but the extremest limit of the land; loitering under the shady lee of yonder warehouses will not suffice. No. They must get just as nigh the water as they possibly can without falling in. And there they stand--miles of them--leagues. Inlanders all, they come from lanes and alleys, streets and avenues--north, east, south, and west. Yet here they all unite. Tell me, does the magnetic virtue of the needles of the compasses of all those ships attract them thither?

Once more. Say you are in the country; in some high land of lakes. Take almost any path you please, and ten to one it carries you down in a dale, and leaves you there by a pool in the stream. There is magic in it. Let the most absent-minded of men be plunged in his deepest reveries--stand that man on his legs, set his feet a-going, and he will infallibly lead you to water, if water there be in all that region. Should you ever be athirst in the great American desert, try this experiment, if your caravan happen to be supplied with a metaphysical professor. Yes, as every one knows, meditation and water are wedded for ever.

But here is an artist. He desires to paint you the dreamiest, shadiest, quietest, most enchanting bit of romantic landscape in all the valley of the Saco. What is the chief element he employs? There stand his trees, each with a hollow trunk, as if a hermit and a crucifix were within; and here sleeps his meadow, and there sleep his cattle; and up from yonder cottage goes a sleepy smoke. Deep into distant woodlands winds a mazy way, reaching to overlapping spurs of mountains bathed in their hill-side blue. But though the picture lies thus tranced, and though this pine-tree shakes down its sighs like leaves upon this shepherd's head, yet all were vain, unless the shepherd's eye were fixed upon the magic stream before him. Go visit the Prairies in June, when for scores on scores of miles you wade knee-deep among Tiger-lilies--what is the one charm wanting?--Water--there is not a drop of water there! Were Niagara but a cataract of sand, would you travel your thousand miles to see it? Why did the poor poet of Tennessee, upon suddenly receiving two handfuls of silver, deliberate whether to buy him a coat, which he sadly needed, or invest his money in a pedestrian trip to Rockaway Beach? Why is almost every robust healthy boy with a robust healthy soul in him, at some time or other crazy to go to sea? Why upon your first voyage as a passenger, did you yourself feel such a mystical vibration, when first told that you and your ship were now out of sight of land? Why did the old Persians hold the sea holy? Why did the Greeks give it a separate deity, and own brother of Jove? Surely all this is not without meaning. And still deeper the meaning of that story of Narcissus, who because he could not grasp the tormenting, mild image he saw in the fountain, plunged into it and was drowned. But that same image, we ourselves see in all rivers and oceans. It is the image of the ungraspable phantom of life; and this is the key to it all.

Now, when I say that I am in the habit of going to sea whenever I begin to grow hazy about the eyes, and begin to be over conscious of my lungs, I do not mean to have it inferred that I ever go to sea as a passenger. For to go as a passenger you must needs have a purse, and a purse is but a rag unless you have something in it. Besides, passengers get sea-sick--grow quarrelsome--don't sleep of nights--do not enjoy themselves much, as a general thing;--no, I never go as a passenger; nor, though I am something of a salt, do I ever go to sea as a Commodore, or a Captain, or a Cook. I abandon the glory and distinction of such offices to those who like them. For my part, I abominate all honourable respectable toils, trials, and tribulations of every kind whatsoever. It is quite as much as I can do to take care of myself, without taking care of ships, barques, brigs, schooners, and what not. And as for going as cook,--though I confess there is considerable glory in that, a cook being a sort of officer on ship-board--yet, somehow, I never fancied broiling fowls;--though once broiled, judiciously buttered, and judgmatically salted and peppered, there is no one who will speak more respectfully, not to say reverentially, of a broiled fowl than I will. It is out of the idolatrous dotings of the old Egyptians upon broiled ibis and roasted river horse, that you see the mummies of those creatures in their huge bake-houses the pyramids.

No, when I go to sea, I go as a simple sailor, right before the mast, plumb down into the forecastle, aloft there to the royal mast-head. True, they rather order me about some, and make me jump from spar to spar, like a grasshopper in a May meadow. And at first, this sort of thing is unpleasant enough. It touches one's sense of honour, particularly if you come of an old established family in the land, the Van Rensselaers, or Randolphs, or Hardicanutes. And more than all, if just previous to putting your hand into the tar-pot, you have been lording it as a country schoolmaster, making the tallest boys stand in awe of you. The transition is a keen one, I assure you, from a schoolmaster to a sailor, and requires a strong decoction of Seneca and the Stoics to enable you to grin and bear it. But even this wears off in time.

What of it, if some old hunks of a sea-captain orders me to get a broom and sweep down the decks? What does that indignity amount to, weighed, I mean, in the scales of the New Testament? Do you think the archangel Gabriel thinks anything the less of me, because I promptly and respectfully obey that old hunks in that particular instance? Who ain't a slave? Tell me that. Well, then, however the old sea-captains may order me about--however they may thump and punch me about, I have the satisfaction of knowing that it is all right; that everybody else is one way or other served in much the same way--either in a physical or metaphysical point of view, that is; and so the universal thump is passed round, and all hands should rub each other's shoulder-blades, and be content.

Again, I always go to sea as a sailor, because they make a point of paying me for my trouble, whereas they never pay passengers a single penny that I ever heard of. On the contrary, passengers themselves must pay. And there is all the difference in the world between paying and being paid. The act of paying is perhaps the most uncomfortable infliction that the two orchard thieves entailed upon us. But BEING PAID,--what will compare with it? The urbane activity with which a man receives money is really marvellous, considering that we so earnestly believe money to be the root of all earthly ills, and that on no account can a monied man enter heaven. Ah! how cheerfully we consign ourselves to perdition!

Finally, I always go to sea as a sailor, because of the wholesome exercise and pure air of the fore-castle deck. For as in this world, head winds are far more prevalent than winds from astern (that is, if you never violate the Pythagorean maxim), so for the most part the Commodore on the quarter-deck gets his atmosphere at second hand from the sailors on the forecastle. He thinks he breathes it first; but not so. In much the same way do the commonalty lead their leaders in many other things, at the same time that the leaders little suspect it. But wherefore it was that after having repeatedly smelt the sea as a merchant sailor, I should now take it into my head to go on a whaling voyage; this the invisible police officer of the Fates, who has the constant surveillance of me, and secretly dogs me, and influences me in some unaccountable way--he can better answer than any one else. And, doubtless, my going on this whaling voyage, formed part of the grand programme of Providence that was drawn up a long time ago. It came in as a sort of brief interlude and solo between more extensive performances. I take it that this part of the bill must have run something like this:

\"GRAND CONTESTED ELECTION FOR THE PRESIDENCY OF THE UNITED STATES. \"WHALING VOYAGE BY ONE ISHMAEL. \"BLOODY BATTLE IN AFFGHANISTAN.\"

Though I cannot tell why it was exactly that those stage managers, the Fates, put me down for this shabby part of a whaling voyage, when others were set down for magnificent parts in high tragedies, and short and easy parts in genteel comedies, and jolly parts in farces--though I cannot tell why this was exactly; yet, now that I recall all the circumstances, I think I can see a little into the springs and motives which being cunningly presented to me under various disguises, induced me to set about performing the part I did, besides cajoling me into the delusion that it was a choice resulting from my own unbiased freewill and discriminating judgment.

Chief among these motives was the overwhelming idea of the great whale himself. Such a portentous and mysterious monster roused all my curiosity. Then the wild and distant seas where he rolled his island bulk; the undeliverable, nameless perils of the whale; these, with all the attending marvels of a thousand Patagonian sights and sounds, helped to sway me to my wish. With other men, perhaps, such things would not have been inducements; but as for me, I am tormented with an everlasting itch for things remote. I love to sail forbidden seas, and land on barbarous coasts. Not ignoring what is good, I am quick to perceive a horror, and could still be social with it--would they let me--since it is but well to be on friendly terms with all the inmates of the place one lodges in.

By reason of these things, then, the whaling voyage was welcome; the great flood-gates of the wonder-world swung open, and in the wild conceits that swayed me to my purpose, two and two there floated into my inmost soul, endless processions of the whale, and, mid most of them all, one grand hooded phantom, like a snow hill in the air.

CHAPTER 2

The Carpet-Bag.

I stuffed a shirt or two into my old carpet-bag, tucked it under my arm, and started for Cape Horn and the Pacific. Quitting the good city of old Manhatto, I duly arrived in New Bedford. It was a Saturday night in December. Much was I disappointed upon learning that the little packet for Nantucket had already sailed, and that no way of reaching that place would offer, till the following Monday.

As most young candidates for the pains and penalties of whaling stop at this same New Bedford, thence to embark on their voyage, it may as well be related that I, for one, had no idea of so doing. For my mind was made up to sail in no other than a Nantucket craft, because there was a fine, boisterous something about everything connected with that famous old island, which amazingly pleased me. Besides though New Bedford has of late been gradually monopolising the business of whaling, and though in this matter poor old Nantucket is now much behind her, yet Nantucket was her great original--the Tyre of this Carthage;--the place where the first dead American whale was stranded. Where else but from Nantucket did those aboriginal whalemen, the Red-Men, first sally out in canoes to give chase to the Leviathan? And where but from Nantucket, too, did that first adventurous little sloop put forth, partly laden with imported cobblestones--so goes the story--to throw at the whales, in order to discover when they were nigh enough to risk a harpoon from the bowsprit?

Now having a night, a day, and still another night following before me in New Bedford, ere I could embark for my destined port, it became a matter of concernment where I was to eat and sleep meanwhile. It was a very dubious-looking, nay, a very dark and dismal night, bitingly cold and cheerless. I knew no one in the place. With anxious grapnels I had sounded my pocket, and only brought up a few pieces of silver,--So, wherever you go, Ishmael, said I to myself, as I stood in the middle of a dreary street shouldering my bag, and comparing the gloom towards the north with the darkness towards the south--wherever in your wisdom you may conclude to lodge for the night, my dear Ishmael, be sure to inquire the price, and don't be too particular.

With halting steps I paced the streets, and passed the sign of \"The Crossed Harpoons\"--but it looked too expensive and jolly there. Further on, from the bright red windows of the \"Sword-Fish Inn,\" there came such fervent rays, that it seemed to have melted the packed snow and ice from before the house, for everywhere else the congealed frost lay ten inches thick in a hard, asphaltic pavement,--rather weary for me, when I struck my foot against the flinty projections, because from hard, remorseless service the soles of my boots were in a most miserable plight. Too expensive and jolly, again thought I, pausing one moment to watch the broad glare in the street, and hear the sounds of the tinkling glasses within. But go on, Ishmael, said I at last; don't you hear? get away from before the door; your patched boots are stopping the way. So on I went. I now by instinct followed the streets that took me waterward, for there, doubtless, were the cheapest, if not the cheeriest inns.

Such dreary streets! blocks of blackness, not houses, on either hand, and here and there a candle, like a candle moving about in a tomb. At this hour of the night, of the last day of the week, that quarter of the town proved all but deserted. But presently I came to a smoky light proceeding from a low, wide building, the door of which stood invitingly open. It had a careless look, as if it were meant for the uses of the public; so, entering, the first thing I did was to stumble over an ash-box in the porch. Ha! thought I, ha, as the flying particles almost choked me, are these ashes from that destroyed city, Gomorrah? But \"The Crossed Harpoons,\" and \"The Sword-Fish?\"--this, then must needs be the sign of \"The Trap.\" However, I picked myself up and hearing a loud voice within, pushed on and opened a second, interior door.

It seemed the great Black Parliament sitting in Tophet. A hundred black faces turned round in their rows to peer; and beyond, a black Angel of Doom was beating a book in a pulpit. It was a negro church; and the preacher's text was about the blackness of darkness, and the weeping and wailing and teeth-gnashing there. Ha, Ishmael, muttered I, backing out, Wretched entertainment at the sign of 'The Trap!'

Moving on, I at last came to a dim sort of light not far from the docks, and heard a forlorn creaking in the air; and looking up, saw a swinging sign over the door with a white painting upon it, faintly representing a tall straight jet of misty spray, and these words underneath--\"The Spouter Inn:--Peter Coffin.\"

Coffin?--Spouter?--Rather ominous in that particular connexion, thought I. But it is a common name in Nantucket, they say, and I suppose this Peter here is an emigrant from there. As the light looked so dim, and the place, for the time, looked quiet enough, and the dilapidated little wooden house itself looked as if it might have been carted here from the ruins of some burnt district, and as the swinging sign had a poverty-stricken sort of creak to it, I thought that here was the very spot for cheap lodgings, and the best of pea coffee.

It was a queer sort of place--a gable-ended old house, one side palsied as it were, and leaning over sadly. It stood on a sharp bleak corner, where that tempestuous wind Euroclydon kept up a worse howling than ever it did about poor Paul's tossed craft. Euroclydon, nevertheless, is a mighty pleasant zephyr to any one in-doors, with his feet on the hob quietly toasting for bed. \"In judging of that tempestuous wind called Euroclydon,\" says an old writer--of whose works I possess the only copy extant--\"it maketh a marvellous difference, whether thou lookest out at it from a glass window where the frost is all on the outside, or whether thou observest it from that sashless window, where the frost is on both sides, and of which the wight Death is the only glazier.\" True enough, thought I, as this passage occurred to my mind--old black-letter, thou reasonest well. Yes, these eyes are windows, and this body of mine is the house. What a pity they didn't stop up the chinks and the crannies though, and thrust in a little lint here and there. But it's too late to make any improvements now. The universe is finished; the copestone is on, and the chips were carted off a million years ago. Poor Lazarus there, chattering his teeth against the curbstone for his pillow, and shaking off his tatters with his shiverings, he might plug up both ears with rags, and put a corn-cob into his mouth, and yet that would not keep out the tempestuous Euroclydon. Euroclydon! says old Dives, in his red silken wrapper--(he had a redder one afterwards) pooh, pooh! What a fine frosty night; how Orion glitters; what northern lights! Let them talk of their oriental summer climes of everlasting conservatories; give me the privilege of making my own summer with my own coals.

But what thinks Lazarus? Can he warm his blue hands by holding them up to the grand northern lights? Would not Lazarus rather be in Sumatra than here? Would he not far rather lay him down lengthwise along the line of the equator; yea, ye gods! go down to the fiery pit itself, in order to keep out this frost?

Now, that Lazarus should lie stranded there on the curbstone before the door of Dives, this is more wonderful than that an iceberg should be moored to one of the Moluccas. Yet Dives himself, he too lives like a Czar in an ice palace made of frozen sighs, and being a president of a temperance society, he only drinks the tepid tears of orphans.

But no more of this blubbering now, we are going a-whaling, and there is plenty of that yet to come. Let us scrape the ice from our frosted feet, and see what sort of a place this \"Spouter\" may be.

CHAPTER 3

The Spouter-Inn.

Entering that gable-ended Spouter-Inn, you found yourself in a wide, low, straggling entry with old-fashioned wainscots, reminding one of the bulwarks of some condemned old craft. On one side hung a very large oilpainting so thoroughly besmoked, and every way defaced, that in the unequal crosslights by which you viewed it, it was only by diligent study and a series of systematic visits to it, and careful inquiry of the neighbors, that you could any way arrive at an understanding of its purpose. Such unaccountable masses of shades and shadows, that at first you almost thought some ambitious young artist, in the time of the New England hags, had endeavored to delineate chaos bewitched. But by dint of much and earnest contemplation, and oft repeated ponderings, and especially by throwing open the little window towards the back of the entry, you at last come to the conclusion that such an idea, however wild, might not be altogether unwarranted.

But what most puzzled and confounded you was a long, limber, portentous, black mass of something hovering in the centre of the picture over three blue, dim, perpendicular lines floating in a nameless yeast. A boggy, soggy, squitchy picture truly, enough to drive a nervous man distracted. Yet was there a sort of indefinite, half-attained, unimaginable sublimity about it that fairly froze you to it, till you involuntarily took an oath with yourself to find out what that marvellous painting meant. Ever and anon a bright, but, alas, deceptive idea would dart you through.--It's the Black Sea in a midnight gale.--It's the unnatural combat of the four primal elements.--It's a blasted heath.--It's a Hyperborean winter scene.--It's the breaking-up of the icebound stream of Time. But at last all these fancies yielded to that one portentous something in the picture's midst. THAT once found out, and all the rest were plain. But stop; does it not bear a faint resemblance to a gigantic fish? even the great leviathan himself?

In fact, the artist's design seemed this: a final theory of my own, partly based upon the aggregated opinions of many aged persons with whom I conversed upon the subject. The picture represents a Cape-Horner in a great hurricane; the half-foundered ship weltering there with its three dismantled masts alone visible; and an exasperated whale, purposing to spring clean over the craft, is in the enormous act of impaling himself upon the three mast-heads.

The opposite wall of this entry was hung all over with a heathenish array of monstrous clubs and spears. Some were thickly set with glittering teeth resembling ivory saws; others were tufted with knots of human hair; and one was sickle-shaped, with a vast handle sweeping round like the segment made in the new-mown grass by a long-armed mower. You shuddered as you gazed, and wondered what monstrous cannibal and savage could ever have gone a death-harvesting with such a hacking, horrifying implement. Mixed with these were rusty old whaling lances and harpoons all broken and deformed. Some were storied weapons. With this once long lance, now wildly elbowed, fifty years ago did Nathan Swain kill fifteen whales between a sunrise and a sunset. And that harpoon--so like a corkscrew now--was flung in Javan seas, and run away with by a whale, years afterwards slain off the Cape of Blanco. The original iron entered nigh the tail, and, like a restless needle sojourning in the body of a man, travelled full forty feet, and at last was found imbedded in the hump.

Crossing this dusky entry, and on through yon low-arched way--cut through what in old times must have been a great central chimney with fireplaces all round--you enter the public room. A still duskier place is this, with such low ponderous beams above, and such old wrinkled planks beneath, that you would almost fancy you trod some old craft's cockpits, especially of such a howling night, when this corner-anchored old ark rocked so furiously. On one side stood a long, low, shelf-like table covered with cracked glass cases, filled with dusty rarities gathered from this wide world's remotest nooks. Projecting from the further angle of the room stands a dark-looking den--the bar--a rude attempt at a right whale's head. Be that how it may, there stands the vast arched bone of the whale's jaw, so wide, a coach might almost drive beneath it. Within are shabby shelves, ranged round with old decanters, bottles, flasks; and in those jaws of swift destruction, like another cursed Jonah (by which name indeed they called him), bustles a little withered old man, who, for their money, dearly sells the sailors deliriums and death.

Abominable are the tumblers into which he pours his poison. Though true cylinders without--within, the villanous green goggling glasses deceitfully tapered downwards to a cheating bottom. Parallel meridians rudely pecked into the glass, surround these footpads' goblets. Fill to THIS mark, and your charge is but a penny; to THIS a penny more; and so on to the full glass--the Cape Horn measure, which you may gulp down for a shilling.

Upon entering the place I found a number of young seamen gathered about a table, examining by a dim light divers specimens of SKRIMSHANDER. I sought the landlord, and telling him I desired to be accommodated with a room, received for answer that his house was full--not a bed unoccupied. \"But avast,\" he added, tapping his forehead, \"you haint no objections to sharing a harpooneer's blanket, have ye? I s'pose you are goin' a-whalin', so you'd better get used to that sort of thing.\"

I told him that I never liked to sleep two in a bed; that if I should ever do so, it would depend upon who the harpooneer might be, and that if he (the landlord) really had no other place for me, and the harpooneer was not decidedly objectionable, why rather than wander further about a strange town on so bitter a night, I would put up with the half of any decent man's blanket.

\"I thought so. All right; take a seat. Supper?--you want supper? Supper'll be ready directly.\"

I sat down on an old wooden settle, carved all over like a bench on the Battery. At one end a ruminating tar was still further adorning it with his jack-knife, stooping over and diligently working away at the space between his legs. He was trying his hand at a ship under full sail, but he didn't make much headway, I thought.

At last some four or five of us were summoned to our meal in an adjoining room. It was cold as Iceland--no fire at all--the landlord said he couldn't afford it. Nothing but two dismal tallow candles, each in a winding sheet. We were fain to button up our monkey jackets, and hold to our lips cups of scalding tea with our half frozen fingers. But the fare was of the most substantial kind--not only meat and potatoes, but dumplings; good heavens! dumplings for supper! One young fellow in a green box coat, addressed himself to these dumplings in a most direful manner.

\"My boy,\" said the landlord, \"you'll have the nightmare to a dead sartainty.\"

\"Landlord,\" I whispered, \"that aint the harpooneer is it?\"

\"Oh, no,\" said he, looking a sort of diabolically funny, \"the harpooneer is a dark complexioned chap. He never eats dumplings, he don't--he eats nothing but steaks, and he likes 'em rare.\"

\"The devil he does,\" says I. \"Where is that harpooneer? Is he here?\"

\"He'll be here afore long,\" was the answer.

I could not help it, but I began to feel suspicious of this \"dark complexioned\" harpooneer. At any rate, I made up my mind that if it so turned out that we should sleep together, he must undress and get into bed before I did.

Supper over, the company went back to the bar-room, when, knowing not what else to do with myself, I resolved to spend the rest of the evening as a looker on.

Presently a rioting noise was heard without. Starting up, the landlord cried, \"That's the Grampus's crew. I seed her reported in the offing this morning; a three years' voyage, and a full ship. Hurrah, boys; now we'll have the latest news from the Feegees.\"

A tramping of sea boots was heard in the entry; the door was flung open, and in rolled a wild set of mariners enough. Enveloped in their shaggy watch coats, and with their heads muffled in woollen comforters, all bedarned and ragged, and their beards stiff with icicles, they seemed an eruption of bears from Labrador. They had just landed from their boat, and this was the first house they entered. No wonder, then, that they made a straight wake for the whale's mouth--the bar--when the wrinkled little old Jonah, there officiating, soon poured them out brimmers all round. One complained of a bad cold in his head, upon which Jonah mixed him a pitch-like potion of gin and molasses, which he swore was a sovereign cure for all colds and catarrhs whatsoever, never mind of how long standing, or whether caught off the coast of Labrador, or on the weather side of an ice-island.

The liquor soon mounted into their heads, as it generally does even with the arrantest topers newly landed from sea, and they began capering about most obstreperously.

I observed, however, that one of them held somewhat aloof, and though he seemed desirous not to spoil the hilarity of his shipmates by his own sober face, yet upon the whole he refrained from making as much noise as the rest. This man interested me at once; and since the sea-gods had ordained that he should soon become my shipmate (though but a sleeping-partner one, so far as this narrative is concerned), I will here venture upon a little description of him. He stood full six feet in height, with noble shoulders, and a chest like a coffer-dam. I have seldom seen such brawn in a man. His face was deeply brown and burnt, making his white teeth dazzling by the contrast; while in the deep shadows of his eyes floated some reminiscences that did not seem to give him much joy. His voice at once announced that he was a Southerner, and from his fine stature, I thought he must be one of those tall mountaineers from the Alleghanian Ridge in Virginia. When the revelry of his companions had mounted to its height, this man slipped away unobserved, and I saw no more of him till he became my comrade on the sea. In a few minutes, however, he was missed by his shipmates, and being, it seems, for some reason a huge favourite with them, they raised a cry of \"Bulkington! Bulkington! where's Bulkington?\" and darted out of the house in pursuit of him.

It was now about nine o'clock, and the room seeming almost supernaturally quiet after these orgies, I began to congratulate myself upon a little plan that had occurred to me just previous to the entrance of the seamen.

No man prefers to sleep two in a bed. In fact, you would a good deal rather not sleep with your own brother. I don't know how it is, but people like to be private when they are sleeping. And when it comes to sleeping with an unknown stranger, in a strange inn, in a strange town, and that stranger a harpooneer, then your objections indefinitely multiply. Nor was there any earthly reason why I as a sailor should sleep two in a bed, more than anybody else; for sailors no more sleep two in a bed at sea, than bachelor Kings do ashore. To be sure they all sleep together in one apartment, but you have your own hammock, and cover yourself with your own blanket, and sleep in your own skin.

The more I pondered over this harpooneer, the more I abominated the thought of sleeping with him. It was fair to presume that being a harpooneer, his linen or woollen, as the case might be, would not be of the tidiest, certainly none of the finest. I began to twitch all over. Besides, it was getting late, and my decent harpooneer ought to be home and going bedwards. Suppose now, he should tumble in upon me at midnight--how could I tell from what vile hole he had been coming?

\"Landlord! I've changed my mind about that harpooneer.--I shan't sleep with him. I'll try the bench here.\"

\"Just as you please; I'm sorry I cant spare ye a tablecloth for a mattress, and it's a plaguy rough board here\"--feeling of the knots and notches. \"But wait a bit, Skrimshander; I've got a carpenter's plane there in the bar--wait, I say, and I'll make ye snug enough.\" So saying he procured the plane; and with his old silk handkerchief first dusting the bench, vigorously set to planing away at my bed, the while grinning like an ape. The shavings flew right and left; till at last the plane-iron came bump against an indestructible knot. The landlord was near spraining his wrist, and I told him for heaven's sake to quit--the bed was soft enough to suit me, and I did not know how all the planing in the world could make eider down of a pine plank. So gathering up the shavings with another grin, and throwing them into the great stove in the middle of the room, he went about his business, and left me in a brown study.

I now took the measure of the bench, and found that it was a foot too short; but that could be mended with a chair. But it was a foot too narrow, and the other bench in the room was about four inches higher than the planed one--so there was no yoking them. I then placed the first bench lengthwise along the only clear space against the wall, leaving a little interval between, for my back to settle down in. But I soon found that there came such a draught of cold air over me from under the sill of the window, that this plan would never do at all, especially as another current from the rickety door met the one from the window, and both together formed a series of small whirlwinds in the immediate vicinity of the spot where I had thought to spend the night.

The devil fetch that harpooneer, thought I, but stop, couldn't I steal a march on him--bolt his door inside, and jump into his bed, not to be wakened by the most violent knockings? It seemed no bad idea; but upon second thoughts I dismissed it. For who could tell but what the next morning, so soon as I popped out of the room, the harpooneer might be standing in the entry, all ready to knock me down!

Still, looking round me again, and seeing no possible chance of spending a sufferable night unless in some other person's bed, I began to think that after all I might be cherishing unwarrantable prejudices against this unknown harpooneer. Thinks I, I'll wait awhile; he must be dropping in before long. I'll have a good look at him then, and perhaps we may become jolly good bedfellows after all--there's no telling.

But though the other boarders kept coming in by ones, twos, and threes, and going to bed, yet no sign of my harpooneer.

\"Landlord! said I, \"what sort of a chap is he--does he always keep such late hours?\" It was now hard upon twelve o'clock.

The landlord chuckled again with his lean chuckle, and seemed to be mightily tickled at something beyond my comprehension. \"No,\" he answered, \"generally he's an early bird--airley to bed and airley to rise--yes, he's the bird what catches the worm. But to-night he went out a peddling, you see, and I don't see what on airth keeps him so late, unless, may be, he can't sell his head.\"

\"Can't sell his head?--What sort of a bamboozingly story is this you are telling me?\" getting into a towering rage. \"Do you pretend to say, landlord, that this harpooneer is actually engaged this blessed Saturday night, or rather Sunday morning, in peddling his head around this town?\"

\"That's precisely it,\" said the landlord, \"and I told him he couldn't sell it here, the market's overstocked.\"

\"With what?\" shouted I.

\"With heads to be sure; ain't there too many heads in the world?\"

\"I tell you what it is, landlord,\" said I quite calmly, \"you'd better stop spinning that yarn to me--I'm not green.\"

\"May be not,\" taking out a stick and whittling a toothpick, \"but I rayther guess you'll be done BROWN if that ere harpooneer hears you a slanderin' his head.\"

\"I'll break it for him,\" said I, now flying into a passion again at this unaccountable farrago of the landlord's.

\"It's broke a'ready,\" said he.

\"Broke,\" said I--\"BROKE, do you mean?\"

\"Sartain, and that's the very reason he can't sell it, I guess.\"

\"Landlord,\" said I, going up to him as cool as Mt. Hecla in a snow-storm--\"landlord, stop whittling. You and I must understand one another, and that too without delay. I come to your house and want a bed; you tell me you can only give me half a one; that the other half belongs to a certain harpooneer. And about this harpooneer, whom I have not yet seen, you persist in telling me the most mystifying and exasperating stories tending to beget in me an uncomfortable feeling towards the man whom you design for my bedfellow--a sort of connexion, landlord, which is an intimate and confidential one in the highest degree. I now demand of you to speak out and tell me who and what this harpooneer is, and whether I shall be in all respects safe to spend the night with him. And in the first place, you will be so good as to unsay that story about selling his head, which if true I take to be good evidence that this harpooneer is stark mad, and I've no idea of sleeping with a madman; and you, sir, YOU I mean, landlord, YOU, sir, by trying to induce me to do so knowingly, would thereby render yourself liable to a criminal prosecution.\"

\"Wall,\" said the landlord, fetching a long breath, \"that's a purty long sarmon for a chap that rips a little now and then. But be easy, be easy, this here harpooneer I have been tellin' you of has just arrived from the south seas, where he bought up a lot of 'balmed New Zealand heads (great curios, you know), and he's sold all on 'em but one, and that one he's trying to sell to-night, cause to-morrow's Sunday, and it would not do to be sellin' human heads about the streets when folks is goin' to churches. He wanted to, last Sunday, but I stopped him just as he was goin' out of the door with four heads strung on a string, for all the airth like a string of inions.\"

This account cleared up the otherwise unaccountable mystery, and showed that the landlord, after all, had had no idea of fooling me--but at the same time what could I think of a harpooneer who stayed out of a Saturday night clean into the holy Sabbath, engaged in such a cannibal business as selling the heads of dead idolators?

\"Depend upon it, landlord, that harpooneer is a dangerous man.\"

\"He pays reg'lar,\" was the rejoinder. \"But come, it's getting dreadful late, you had better be turning flukes--it's a nice bed; Sal and me slept in that ere bed the night we were spliced. There's plenty of room for two to kick about in that bed; it's an almighty big bed that. Why, afore we give it up, Sal used to put our Sam and little Johnny in the foot of it. But I got a dreaming and sprawling about one night, and somehow, Sam got pitched on the floor, and came near breaking his arm. Arter that, Sal said it wouldn't do. Come along here, I'll give ye a glim in a jiffy;\" and so saying he lighted a candle and held it towards me, offering to lead the way. But I stood irresolute; when looking at a clock in the corner, he exclaimed \"I vum it's Sunday--you won't see that harpooneer to-night; he's come to anchor somewhere--come along then; DO come; WON'T ye come?\"

I considered the matter a moment, and then up stairs we went, and I was ushered into a small room, cold as a clam, and furnished, sure enough, with a prodigious bed, almost big enough indeed for any four harpooneers to sleep abreast.

\"There,\" said the landlord, placing the candle on a crazy old sea chest that did double duty as a wash-stand and centre table; \"there, make yourself comfortable now, and good night to ye.\" I turned round from eyeing the bed, but he had disappeared.

Folding back the counterpane, I stooped over the bed. Though none of the most elegant, it yet stood the scrutiny tolerably well. I then glanced round the room; and besides the bedstead and centre table, could see no other furniture belonging to the place, but a rude shelf, the four walls, and a papered fireboard representing a man striking a whale. Of things not properly belonging to the room, there was a hammock lashed up, and thrown upon the floor in one corner; also a large seaman's bag, containing the harpooneer's wardrobe, no doubt in lieu of a land trunk. Likewise, there was a parcel of outlandish bone fish hooks on the shelf over the fire-place, and a tall harpoon standing at the head of the bed.

But what is this on the chest? I took it up, and held it close to the light, and felt it, and smelt it, and tried every way possible to arrive at some satisfactory conclusion concerning it. I can compare it to nothing but a large door mat, ornamented at the edges with little tinkling tags something like the stained porcupine quills round an Indian moccasin. There was a hole or slit in the middle of this mat, as you see the same in South American ponchos. But could it be possible that any sober harpooneer would get into a door mat, and parade the streets of any Christian town in that sort of guise? I put it on, to try it, and it weighed me down like a hamper, being uncommonly shaggy and thick, and I thought a little damp, as though this mysterious harpooneer had been wearing it of a rainy day. I went up in it to a bit of glass stuck against the wall, and I never saw such a sight in my life. I tore myself out of it in such a hurry that I gave myself a kink in the neck.

I sat down on the side of the bed, and commenced thinking about this head-peddling harpooneer, and his door mat. After thinking some time on the bed-side, I got up and took off my monkey jacket, and then stood in the middle of the room thinking. I then took off my coat, and thought a little more in my shirt sleeves. But beginning to feel very cold now, half undressed as I was, and remembering what the landlord said about the harpooneer's not coming home at all that night, it being so very late, I made no more ado, but jumped out of my pantaloons and boots, and then blowing out the light tumbled into bed, and commended myself to the care of heaven.

Whether that mattress was stuffed with corn-cobs or broken crockery, there is no telling, but I rolled about a good deal, and could not sleep for a long time. At last I slid off into a light doze, and had pretty nearly made a good offing towards the land of Nod, when I heard a heavy footfall in the passage, and saw a glimmer of light come into the room from under the door.

Lord save me, thinks I, that must be the harpooneer, the infernal head-peddler. But I lay perfectly still, and resolved not to say a word till spoken to. Holding a light in one hand, and that identical New Zealand head in the other, the stranger entered the room, and without looking towards the bed, placed his candle a good way off from me on the floor in one corner, and then began working away at the knotted cords of the large bag I before spoke of as being in the room. I was all eagerness to see his face, but he kept it averted for some time while employed in unlacing the bag's mouth. This accomplished, however, he turned round--when, good heavens! what a sight! Such a face! It was of a dark, purplish, yellow colour, here and there stuck over with large blackish looking squares. Yes, it's just as I thought, he's a terrible bedfellow; he's been in a fight, got dreadfully cut, and here he is, just from the surgeon. But at that moment he chanced to turn his face so towards the light, that I plainly saw they could not be sticking-plasters at all, those black squares on his cheeks. They were stains of some sort or other. At first I knew not what to make of this; but soon an inkling of the truth occurred to me. I remembered a story of a white man--a whaleman too--who, falling among the cannibals, had been tattooed by them. I concluded that this harpooneer, in the course of his distant voyages, must have met with a similar adventure. And what is it, thought I, after all! It's only his outside; a man can be honest in any sort of skin. But then, what to make of his unearthly complexion, that part of it, I mean, lying round about, and completely independent of the squares of tattooing. To be sure, it might be nothing but a good coat of tropical tanning; but I never heard of a hot sun's tanning a white man into a purplish yellow one. However, I had never been in the South Seas; and perhaps the sun there produced these extraordinary effects upon the skin. Now, while all these ideas were passing through me like lightning, this harpooneer never noticed me at all. But, after some difficulty having opened his bag, he commenced fumbling in it, and presently pulled out a sort of tomahawk, and a seal-skin wallet with the hair on. Placing these on the old chest in the middle of the room, he then took the New Zealand head--a ghastly thing enough--and crammed it down into the bag. He now took off his hat--a new beaver hat--when I came nigh singing out with fresh surprise. There was no hair on his head--none to speak of at least--nothing but a small scalp-knot twisted up on his forehead. His bald purplish head now looked for all the world like a mildewed skull. Had not the stranger stood between me and the door, I would have bolted out of it quicker than ever I bolted a dinner.

Even as it was, I thought something of slipping out of the window, but it was the second floor back. I am no coward, but what to make of this head-peddling purple rascal altogether passed my comprehension. Ignorance is the parent of fear, and being completely nonplussed and confounded about the stranger, I confess I was now as much afraid of him as if it was the devil himself who had thus broken into my room at the dead of night. In fact, I was so afraid of him that I was not game enough just then to address him, and demand a satisfactory answer concerning what seemed inexplicable in him.

Meanwhile, he continued the business of undressing, and at last showed his chest and arms. As I live, these covered parts of him were checkered with the same squares as his face; his back, too, was all over the same dark squares; he seemed to have been in a Thirty Years' War, and just escaped from it with a sticking-plaster shirt. Still more, his very legs were marked, as if a parcel of dark green frogs were running up the trunks of young palms. It was now quite plain that he must be some abominable savage or other shipped aboard of a whaleman in the South Seas, and so landed in this Christian country. I quaked to think of it. A peddler of heads too--perhaps the heads of his own brothers. He might take a fancy to mine--heavens! look at that tomahawk!

But there was no time for shuddering, for now the savage went about something that completely fascinated my attention, and convinced me that he must indeed be a heathen. Going to his heavy grego, or wrapall, or dreadnaught, which he had previously hung on a chair, he fumbled in the pockets, and produced at length a curious little deformed image with a hunch on its back, and exactly the colour of a three days' old Congo baby. Remembering the embalmed head, at first I almost thought that this black manikin was a real baby preserved in some similar manner. But seeing that it was not at all limber, and that it glistened a good deal like polished ebony, I concluded that it must be nothing but a wooden idol, which indeed it proved to be. For now the savage goes up to the empty fire-place, and removing the papered fire-board, sets up this little hunch-backed image, like a tenpin, between the andirons. The chimney jambs and all the bricks inside were very sooty, so that I thought this fire-place made a very appropriate little shrine or chapel for his Congo idol.

I now screwed my eyes hard towards the half hidden image, feeling but ill at ease meantime--to see what was next to follow. First he takes about a double handful of shavings out of his grego pocket, and places them carefully before the idol; then laying a bit of ship biscuit on top and applying the flame from the lamp, he kindled the shavings into a sacrificial blaze. Presently, after many hasty snatches into the fire, and still hastier withdrawals of his fingers (whereby he seemed to be scorching them badly), he at last succeeded in drawing out the biscuit; then blowing off the heat and ashes a little, he made a polite offer of it to the little negro. But the little devil did not seem to fancy such dry sort of fare at all; he never moved his lips. All these strange antics were accompanied by still stranger guttural noises from the devotee, who seemed to be praying in a sing-song or else singing some pagan psalmody or other, during which his face twitched about in the most unnatural manner. At last extinguishing the fire, he took the idol up very unceremoniously, and bagged it again in his grego pocket as carelessly as if he were a sportsman bagging a dead woodcock.

All these queer proceedings increased my uncomfortableness, and seeing him now exhibiting strong symptoms of concluding his business operations, and jumping into bed with me, I thought it was high time, now or never, before the light was put out, to break the spell in which I had so long been bound.

But the interval I spent in deliberating what to say, was a fatal one. Taking up his tomahawk from the table, he examined the head of it for an instant, and then holding it to the light, with his mouth at the handle, he puffed out great clouds of tobacco smoke. The next moment the light was extinguished, and this wild cannibal, tomahawk between his teeth, sprang into bed with me. I sang out, I could not help it now; and giving a sudden grunt of astonishment he began feeling me.

Stammering out something, I knew not what, I rolled away from him against the wall, and then conjured him, whoever or whatever he might be, to keep quiet, and let me get up and light the lamp again. But his guttural responses satisfied me at once that he but ill comprehended my meaning.

\"Who-e debel you?\"--he at last said--\"you no speak-e, dam-me, I kill-e.\" And so saying the lighted tomahawk began flourishing about me in the dark.

\"Landlord, for God's sake, Peter Coffin!\" shouted I. \"Landlord! Watch! Coffin! Angels! save me!\"

\"Speak-e! tell-ee me who-ee be, or dam-me, I kill-e!\" again growled the cannibal, while his horrid flourishings of the tomahawk scattered the hot tobacco ashes about me till I thought my linen would get on fire. But thank heaven, at that moment the landlord came into the room light in hand, and leaping from the bed I ran up to him.

\"Don't be afraid now,\" said he, grinning again, \"Queequeg here wouldn't harm a hair of your head.\"

\"Stop your grinning,\" shouted I, \"and why didn't you tell me that that infernal harpooneer was a cannibal?\"

\"I thought ye know'd it;--didn't I tell ye, he was a peddlin' heads around town?--but turn flukes again and go to sleep. Queequeg, look here--you sabbee me, I sabbee--you this man sleepe you--you sabbee?\"

\"Me sabbee plenty\"--grunted Queequeg, puffing away at his pipe and sitting up in bed.

\"You gettee in,\" he added, motioning to me with his tomahawk, and throwing the clothes to one side. He really did this in not only a civil but a really kind and charitable way. I stood looking at him a moment. For all his tattooings he was on the whole a clean, comely looking cannibal. What's all this fuss I have been making about, thought I to myself--the man's a human being just as I am: he has just as much reason to fear me, as I have to be afraid of him. Better sleep with a sober cannibal than a drunken Christian.

\"Landlord,\" said I, \"tell him to stash his tomahawk there, or pipe, or whatever you call it; tell him to stop smoking, in short, and I will turn in with him. But I don't fancy having a man smoking in bed with me. It's dangerous. Besides, I ain't insured.\"

This being told to Queequeg, he at once complied, and again politely motioned me to get into bed--rolling over to one side as much as to say--I won't touch a leg of ye.\"

\"Good night, landlord,\" said I, \"you may go.\"

I turned in, and never slept better in my life.

CHAPTER 4

The Counterpane.

Upon waking next morning about daylight, I found Queequeg's arm thrown over me in the most loving and affectionate manner. You had almost thought I had been his wife. The counterpane was of patchwork, full of odd little parti-coloured squares and triangles; and this arm of his tattooed all over with an interminable Cretan labyrinth of a figure, no two parts of which were of one precise shade--owing I suppose to his keeping his arm at sea unmethodically in sun and shade, his shirt sleeves irregularly rolled up at various times--this same arm of his, I say, looked for all the world like a strip of that same patchwork quilt. Indeed, partly lying on it as the arm did when I first awoke, I could hardly tell it from the quilt, they so blended their hues together; and it was only by the sense of weight and pressure that I could tell that Queequeg was hugging me.

My sensations were strange. Let me try to explain them. When I was a child, I well remember a somewhat similar circumstance that befell me; whether it was a reality or a dream, I never could entirely settle. The circumstance was this. I had been cutting up some caper or other--I think it was trying to crawl up the chimney, as I had seen a little sweep do a few days previous; and my stepmother who, somehow or other, was all the time whipping me, or sending me to bed supperless,--my mother dragged me by the legs out of the chimney and packed me off to bed, though it was only two o'clock in the afternoon of the 21st June, the longest day in the year in our hemisphere. I felt dreadfully. But there was no help for it, so up stairs I went to my little room in the third floor, undressed myself as slowly as possible so as to kill time, and with a bitter sigh got between the sheets.

I lay there dismally calculating that sixteen entire hours must elapse before I could hope for a resurrection. Sixteen hours in bed! the small of my back ached to think of it. And it was so light too; the sun shining in at the window, and a great rattling of coaches in the streets, and the sound of gay voices all over the house. I felt worse and worse--at last I got up, dressed, and softly going down in my stockinged feet, sought out my stepmother, and suddenly threw myself at her feet, beseeching her as a particular favour to give me a good slippering for my misbehaviour; anything indeed but condemning me to lie abed such an unendurable length of time. But she was the best and most conscientious of stepmothers, and back I had to go to my room. For several hours I lay there broad awake, feeling a great deal worse than I have ever done since, even from the greatest subsequent misfortunes. At last I must have fallen into a troubled nightmare of a doze; and slowly waking from it--half steeped in dreams--I opened my eyes, and the before sun-lit room was now wrapped in outer darkness. Instantly I felt a shock running through all my frame; nothing was to be seen, and nothing was to be heard; but a supernatural hand seemed placed in mine. My arm hung over the counterpane, and the nameless, unimaginable, silent form or phantom, to which the hand belonged, seemed closely seated by my bed-side. For what seemed ages piled on ages, I lay there, frozen with the most awful fears, not daring to drag away my hand; yet ever thinking that if I could but stir it one single inch, the horrid spell would be broken. I knew not how this consciousness at last glided away from me; but waking in the morning, I shudderingly remembered it all, and for days and weeks and months afterwards I lost myself in confounding attempts to explain the mystery. Nay, to this very hour, I often puzzle myself with it.

Now, take away the awful fear, and my sensations at feeling the supernatural hand in mine were very similar, in their strangeness, to those which I experienced on waking up and seeing Queequeg's pagan arm thrown round me. But at length all the past night's events soberly recurred, one by one, in fixed reality, and then I lay only alive to the comical predicament. For though I tried to move his arm--unlock his bridegroom clasp--yet, sleeping as he was, he still hugged me tightly, as though naught but death should part us twain. I now strove to rouse him--\"Queequeg!\"--but his only answer was a snore. I then rolled over, my neck feeling as if it were in a horse-collar; and suddenly felt a slight scratch. Throwing aside the counterpane, there lay the tomahawk sleeping by the savage's side, as if it were a hatchet-faced baby. A pretty pickle, truly, thought I; abed here in a strange house in the broad day, with a cannibal and a tomahawk! \"Queequeg!--in the name of goodness, Queequeg, wake!\" At length, by dint of much wriggling, and loud and incessant expostulations upon the unbecomingness of his hugging a fellow male in that matrimonial sort of style, I succeeded in extracting a grunt; and presently, he drew back his arm, shook himself all over like a Newfoundland dog just from the water, and sat up in bed, stiff as a pike-staff, looking at me, and rubbing his eyes as if he did not altogether remember how I came to be there, though a dim consciousness of knowing something about me seemed slowly dawning over him. Meanwhile, I lay quietly eyeing him, having no serious misgivings now, and bent upon narrowly observing so curious a creature. When, at last, his mind seemed made up touching the character of his bedfellow, and he became, as it were, reconciled to the fact; he jumped out upon the floor, and by certain signs and sounds gave me to understand that, if it pleased me, he would dress first and then leave me to dress afterwards, leaving the whole apartment to myself. Thinks I, Queequeg, under the circumstances, this is a very civilized overture; but, the truth is, these savages have an innate sense of delicacy, say what you will; it is marvellous how essentially polite they are. I pay this particular compliment to Queequeg, because he treated me with so much civility and consideration, while I was guilty of great rudeness; staring at him from the bed, and watching all his toilette motions; for the time my curiosity getting the better of my breeding. Nevertheless, a man like Queequeg you don't see every day, he and his ways were well worth unusual regarding.

He commenced dressing at top by donning his beaver hat, a very tall one, by the by, and then--still minus his trowsers--he hunted up his boots. What under the heavens he did it for, I cannot tell, but his next movement was to crush himself--boots in hand, and hat on--under the bed; when, from sundry violent gaspings and strainings, I inferred he was hard at work booting himself; though by no law of propriety that I ever heard of, is any man required to be private when putting on his boots. But Queequeg, do you see, was a creature in the transition stage--neither caterpillar nor butterfly. He was just enough civilized to show off his outlandishness in the strangest possible manners. His education was not yet completed. He was an undergraduate. If he had not been a small degree civilized, he very probably would not have troubled himself with boots at all; but then, if he had not been still a savage, he never would have dreamt of getting under the bed to put them on. At last, he emerged with his hat very much dented and crushed down over his eyes, and began creaking and limping about the room, as if, not being much accustomed to boots, his pair of damp, wrinkled cowhide ones--probably not made to order either--rather pinched and tormented him at the first go off of a bitter cold morning.

Seeing, now, that there were no curtains to the window, and that the street being very narrow, the house opposite commanded a plain view into the room, and observing more and more the indecorous figure that Queequeg made, staving about with little else but his hat and boots on; I begged him as well as I could, to accelerate his toilet somewhat, and particularly to get into his pantaloons as soon as possible. He complied, and then proceeded to wash himself. At that time in the morning any Christian would have washed his face; but Queequeg, to my amazement, contented himself with restricting his ablutions to his chest, arms, and hands. He then donned his waistcoat, and taking up a piece of hard soap on the wash-stand centre table, dipped it into water and commenced lathering his face. I was watching to see where he kept his razor, when lo and behold, he takes the harpoon from the bed corner, slips out the long wooden stock, unsheathes the head, whets it a little on his boot, and striding up to the bit of mirror against the wall, begins a vigorous scraping, or rather harpooning of his cheeks. Thinks I, Queequeg, this is using Rogers's best cutlery with a vengeance. Afterwards I wondered the less at this operation when I came to know of what fine steel the head of a harpoon is made, and how exceedingly sharp the long straight edges are always kept.

The rest of his toilet was soon achieved, and he proudly marched out of the room, wrapped up in his great pilot monkey jacket, and sporting his harpoon like a marshal's baton.

CHAPTER 5

Breakfast.

I quickly followed suit, and descending into the bar-room accosted the grinning landlord very pleasantly. I cherished no malice towards him, though he had been skylarking with me not a little in the matter of my bedfellow.

However, a good laugh is a mighty good thing, and rather too scarce a good thing; the more's the pity. So, if any one man, in his own proper person, afford stuff for a good joke to anybody, let him not be backward, but let him cheerfully allow himself to spend and be spent in that way. And the man that has anything bountifully laughable about him, be sure there is more in that man than you perhaps think for.

The bar-room was now full of the boarders who had been dropping in the night previous, and whom I had not as yet had a good look at. They were nearly all whalemen; chief mates, and second mates, and third mates, and sea carpenters, and sea coopers, and sea blacksmiths, and harpooneers, and ship keepers; a brown and brawny company, with bosky beards; an unshorn, shaggy set, all wearing monkey jackets for morning gowns.

You could pretty plainly tell how long each one had been ashore. This young fellow's healthy cheek is like a sun-toasted pear in hue, and would seem to smell almost as musky; he cannot have been three days landed from his Indian voyage. That man next him looks a few shades lighter; you might say a touch of satin wood is in him. In the complexion of a third still lingers a tropic tawn, but slightly bleached withal; HE doubtless has tarried whole weeks ashore. But who could show a cheek like Queequeg? which, barred with various tints, seemed like the Andes' western slope, to show forth in one array, contrasting climates, zone by zone.

\"Grub, ho!\" now cried the landlord, flinging open a door, and in we went to breakfast.

They say that men who have seen the world, thereby become quite at ease in manner, quite self-possessed in company. Not always, though: Ledyard, the great New England traveller, and Mungo Park, the Scotch one; of all men, they possessed the least assurance in the parlor. But perhaps the mere crossing of Siberia in a sledge drawn by dogs as Ledyard did, or the taking a long solitary walk on an empty stomach, in the negro heart of Africa, which was the sum of poor Mungo's performances--this kind of travel, I say, may not be the very best mode of attaining a high social polish. Still, for the most part, that sort of thing is to be had anywhere.

These reflections just here are occasioned by the circumstance that after we were all seated at the table, and I was preparing to hear some good stories about whaling; to my no small surprise, nearly every man maintained a profound silence. And not only that, but they looked embarrassed. Yes, here were a set of sea-dogs, many of whom without the slightest bashfulness had boarded great whales on the high seas--entire strangers to them--and duelled them dead without winking; and yet, here they sat at a social breakfast table--all of the same calling, all of kindred tastes--looking round as sheepishly at each other as though they had never been out of sight of some sheepfold among the Green Mountains. A curious sight; these bashful bears, these timid warrior whalemen!

But as for Queequeg--why, Queequeg sat there among them--at the head of the table, too, it so chanced; as cool as an icicle. To be sure I cannot say much for his breeding. His greatest admirer could not have cordially justified his bringing his harpoon into breakfast with him, and using it there without ceremony; reaching over the table with it, to the imminent jeopardy of many heads, and grappling the beefsteaks towards him. But THAT was certainly very coolly done by him, and every one knows that in most people's estimation, to do anything coolly is to do it genteelly.

We will not speak of all Queequeg's peculiarities here; how he eschewed coffee and hot rolls, and applied his undivided attention to beefsteaks, done rare. Enough, that when breakfast was over he withdrew like the rest into the public room, lighted his tomahawk-pipe, and was sitting there quietly digesting and smoking with his inseparable hat on, when I sallied out for a stroll.

CHAPTER 6

The Street.

If I had been astonished at first catching a glimpse of so outlandish an individual as Queequeg circulating among the polite society of a civilized town, that astonishment soon departed upon taking my first daylight stroll through the streets of New Bedford.

In thoroughfares nigh the docks, any considerable seaport will frequently offer to view the queerest looking nondescripts from foreign parts. Even in Broadway and Chestnut streets, Mediterranean mariners will sometimes jostle the affrighted ladies. Regent Street is not unknown to Lascars and Malays; and at Bombay, in the Apollo Green, live Yankees have often scared the natives. But New Bedford beats all Water Street and Wapping. In these last-mentioned haunts you see only sailors; but in New Bedford, actual cannibals stand chatting at street corners; savages outright; many of whom yet carry on their bones unholy flesh. It makes a stranger stare.

But, besides the Feegeeans, Tongatobooarrs, Erromanggoans, Pannangians, and Brighggians, and, besides the wild specimens of the whaling-craft which unheeded reel about the streets, you will see other sights still more curious, certainly more comical. There weekly arrive in this town scores of green Vermonters and New Hampshire men, all athirst for gain and glory in the fishery. They are mostly young, of stalwart frames; fellows who have felled forests, and now seek to drop the axe and snatch the whale-lance. Many are as green as the Green Mountains whence they came. In some things you would think them but a few hours old. Look there! that chap strutting round the corner. He wears a beaver hat and swallow-tailed coat, girdled with a sailor-belt and sheath-knife. Here comes another with a sou'-wester and a bombazine cloak.

No town-bred dandy will compare with a country-bred one--I mean a downright bumpkin dandy--a fellow that, in the dog-days, will mow his two acres in buckskin gloves for fear of tanning his hands. Now when a country dandy like this takes it into his head to make a distinguished reputation, and joins the great whale-fishery, you should see the comical things he does upon reaching the seaport. In bespeaking his sea-outfit, he orders bell-buttons to his waistcoats; straps to his canvas trowsers. Ah, poor Hay-Seed! how bitterly will burst those straps in the first howling gale, when thou art driven, straps, buttons, and all, down the throat of the tempest.

But think not that this famous town has only harpooneers, cannibals, and bumpkins to show her visitors. Not at all. Still New Bedford is a queer place. Had it not been for us whalemen, that tract of land would this day perhaps have been in as howling condition as the coast of Labrador. As it is, parts of her back country are enough to frighten one, they look so bony. The town itself is perhaps the dearest place to live in, in all New England. It is a land of oil, true enough: but not like Canaan; a land, also, of corn and wine. The streets do not run with milk; nor in the spring-time do they pave them with fresh eggs. Yet, in spite of this, nowhere in all America will you find more patrician-like houses; parks and gardens more opulent, than in New Bedford. Whence came they? how planted upon this once scraggy scoria of a country?

Go and gaze upon the iron emblematical harpoons round yonder lofty mansion, and your question will be answered. Yes; all these brave houses and flowery gardens came from the Atlantic, Pacific, and Indian oceans. One and all, they were harpooned and dragged up hither from the bottom of the sea. Can Herr Alexander perform a feat like that?

In New Bedford, fathers, they say, give whales for dowers to their daughters, and portion off their nieces with a few porpoises a-piece. You must go to New Bedford to see a brilliant wedding; for, they say, they have reservoirs of oil in every house, and every night recklessly burn their lengths in spermaceti candles.

In summer time, the town is sweet to see; full of fine maples--long avenues of green and gold. And in August, high in air, the beautiful and bountiful horse-chestnuts, candelabra-wise, proffer the passer-by their tapering upright cones of congregated blossoms. So omnipotent is art; which in many a district of New Bedford has superinduced bright terraces of flowers upon the barren refuse rocks thrown aside at creation's final day.

And the women of New Bedford, they bloom like their own red roses. But roses only bloom in summer; whereas the fine carnation of their cheeks is perennial as sunlight in the seventh heavens. Elsewhere match that bloom of theirs, ye cannot, save in Salem, where they tell me the young girls breathe such musk, their sailor sweethearts smell them miles off shore, as though they were drawing nigh the odorous Moluccas instead of the Puritanic sands.

CHAPTER 7

The Chapel.

In this same New Bedford there stands a Whaleman's Chapel, and few are the moody fishermen, shortly bound for the Indian Ocean or Pacific, who fail to make a Sunday visit to the spot. I am sure that I did not.

Returning from my first morning stroll, I again sallied out upon this special errand. The sky had changed from clear, sunny cold, to driving sleet and mist. Wrapping myself in my shaggy jacket of the cloth called bearskin, I fought my way against the stubborn storm. Entering, I found a small scattered congregation of sailors, and sailors' wives and widows. A muffled silence reigned, only broken at times by the shrieks of the storm. Each silent worshipper seemed purposely sitting apart from the other, as if each silent grief were insular and incommunicable. The chaplain had not yet arrived; and there these silent islands of men and women sat steadfastly eyeing several marble tablets, with black borders, masoned into the wall on either side the pulpit. Three of them ran something like the following, but I do not pretend to quote:--

SACRED TO THE MEMORY OF JOHN TALBOT, Who, at the age of eighteen, was lost overboard, Near the Isle of Desolation, off Patagonia, November 1st, 1836. THIS TABLET Is erected to his Memory BY HIS SISTER.

SACRED TO THE MEMORY OF ROBERT LONG, WILLIS ELLERY, NATHAN COLEMAN, WALTER CANNY, SETH MACY, AND SAMUEL GLEIG, Forming one of the boats' crews OF THE SHIP ELIZA Who were towed out of sight by a Whale, On the Off-shore Ground in the PACIFIC, December 31st, 1839. THIS MARBLE Is here placed by their surviving SHIPMATES.

SACRED TO THE MEMORY OF The late CAPTAIN EZEKIEL HARDY, Who in the bows of his boat was killed by a Sperm Whale on the coast of Japan, AUGUST 3d, 1833. THIS TABLET Is erected to his Memory BY HIS WIDOW.

Shaking off the sleet from my ice-glazed hat and jacket, I seated myself near the door, and turning sideways was surprised to see Queequeg near me. Affected by the solemnity of the scene, there was a wondering gaze of incredulous curiosity in his countenance. This savage was the only person present who seemed to notice my entrance; because he was the only one who could not read, and, therefore, was not reading those frigid inscriptions on the wall. Whether any of the relatives of the seamen whose names appeared there were now among the congregation, I knew not; but so many are the unrecorded accidents in the fishery, and so plainly did several women present wear the countenance if not the trappings of some unceasing grief, that I feel sure that here before me were assembled those, in whose unhealing hearts the sight of those bleak tablets sympathetically caused the old wounds to bleed afresh.

Oh! ye whose dead lie buried beneath the green grass; who standing among flowers can say--here, HERE lies my beloved; ye know not the desolation that broods in bosoms like these. What bitter blanks in those black-bordered marbles which cover no ashes! What despair in those immovable inscriptions! What deadly voids and unbidden infidelities in the lines that seem to gnaw upon all Faith, and refuse resurrections to the beings who have placelessly perished without a grave. As well might those tablets stand in the cave of Elephanta as here.

In what census of living creatures, the dead of mankind are included; why it is that a universal proverb says of them, that they tell no tales, though containing more secrets than the Goodwin Sands; how it is that to his name who yesterday departed for the other world, we prefix so significant and infidel a word, and yet do not thus entitle him, if he but embarks for the remotest Indies of this living earth; why the Life Insurance Companies pay death-forfeitures upon immortals; in what eternal, unstirring paralysis, and deadly, hopeless trance, yet lies antique Adam who died sixty round centuries ago; how it is that we still refuse to be comforted for those who we nevertheless maintain are dwelling in unspeakable bliss; why all the living so strive to hush all the dead; wherefore but the rumor of a knocking in a tomb will terrify a whole city. All these things are not without their meanings.

But Faith, like a jackal, feeds among the tombs, and even from these dead doubts she gathers her most vital hope.

It needs scarcely to be told, with what feelings, on the eve of a Nantucket voyage, I regarded those marble tablets, and by the murky light of that darkened, doleful day read the fate of the whalemen who had gone before me. Yes, Ishmael, the same fate may be thine. But somehow I grew merry again. Delightful inducements to embark, fine chance for promotion, it seems--aye, a stove boat will make me an immortal by brevet. Yes, there is death in this business of whaling--a speechlessly quick chaotic bundling of a man into Eternity. But what then? Methinks we have hugely mistaken this matter of Life and Death. Methinks that what they call my shadow here on earth is my true substance. Methinks that in looking at things spiritual, we are too much like oysters observing the sun through the water, and thinking that thick water the thinnest of air. Methinks my body is but the lees of my better being. In fact take my body who will, take it I say, it is not me. And therefore three cheers for Nantucket; and come a stove boat and stove body when they will, for stave my soul, Jove himself cannot.

CHAPTER 8

The Pulpit.

I had not been seated very long ere a man of a certain venerable robustness entered; immediately as the storm-pelted door flew back upon admitting him, a quick regardful eyeing of him by all the congregation, sufficiently attested that this fine old man was the chaplain. Yes, it was the famous Father Mapple, so called by the whalemen, among whom he was a very great favourite. He had been a sailor and a harpooneer in his youth, but for many years past had dedicated his life to the ministry. At the time I now write of, Father Mapple was in the hardy winter of a healthy old age; that sort of old age which seems merging into a second flowering youth, for among all the fissures of his wrinkles, there shone certain mild gleams of a newly developing bloom--the spring verdure peeping forth even beneath February's snow. No one having previously heard his history, could for the first time behold Father Mapple without the utmost interest, because there were certain engrafted clerical peculiarities about him, imputable to that adventurous maritime life he had led. When he entered I observed that he carried no umbrella, and certainly had not come in his carriage, for his tarpaulin hat ran down with melting sleet, and his great pilot cloth jacket seemed almost to drag him to the floor with the weight of the water it had absorbed. However, hat and coat and overshoes were one by one removed, and hung up in a little space in an adjacent corner; when, arrayed in a decent suit, he quietly approached the pulpit.

Like most old fashioned pulpits, it was a very lofty one, and since a regular stairs to such a height would, by its long angle with the floor, seriously contract the already small area of the chapel, the architect, it seemed, had acted upon the hint of Father Mapple, and finished the pulpit without a stairs, substituting a perpendicular side ladder, like those used in mounting a ship from a boat at sea. The wife of a whaling captain had provided the chapel with a handsome pair of red worsted man-ropes for this ladder, which, being itself nicely headed, and stained with a mahogany colour, the whole contrivance, considering what manner of chapel it was, seemed by no means in bad taste. Halting for an instant at the foot of the ladder, and with both hands grasping the ornamental knobs of the man-ropes, Father Mapple cast a look upwards, and then with a truly sailor-like but still reverential dexterity, hand over hand, mounted the steps as if ascending the main-top of his vessel.

The perpendicular parts of this side ladder, as is usually the case with swinging ones, were of cloth-covered rope, only the rounds were of wood, so that at every step there was a joint. At my first glimpse of the pulpit, it had not escaped me that however convenient for a ship, these joints in the present instance seemed unnecessary. For I was not prepared to see Father Mapple after gaining the height, slowly turn round, and stooping over the pulpit, deliberately drag up the ladder step by step, till the whole was deposited within, leaving him impregnable in his little Quebec.

I pondered some time without fully comprehending the reason for this. Father Mapple enjoyed such a wide reputation for sincerity and sanctity, that I could not suspect him of courting notoriety by any mere tricks of the stage. No, thought I, there must be some sober reason for this thing; furthermore, it must symbolize something unseen. Can it be, then, that by that act of physical isolation, he signifies his spiritual withdrawal for the time, from all outward worldly ties and connexions? Yes, for replenished with the meat and wine of the word, to the faithful man of God, this pulpit, I see, is a self-containing stronghold--a lofty Ehrenbreitstein, with a perennial well of water within the walls.
    ";


}
